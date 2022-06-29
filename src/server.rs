use std::{net::ToSocketAddrs, path::Path, sync::Arc};

use anyhow::{bail, Context};
use tokio::{
    sync::{mpsc, Semaphore},
    time,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

use futures::FutureExt;
use futures::{pin_mut, stream::SplitSink, SinkExt, StreamExt};
use log::{debug, error, info};
use tempdir::TempDir;
use warp::{
    ws::{Message, WebSocket},
    Filter,
};

mod editor;
mod file;
mod msg;
#[cfg(all(feature = "systemd", target_os = "linux"))]
mod systemd;
mod text;
#[cfg(feature = "watch_changes")]
mod watch_changes;

use crate::settings::Settings;

type WebSocketTx = SplitSink<WebSocket, Message>;

#[derive(Debug, Clone)]
struct State {
    options: Settings,
    single_access: Arc<Semaphore>,
}

fn with_state<S: Clone + Send>(
    state: S,
) -> impl Filter<Extract = (S,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

pub async fn run(options: Settings) -> anyhow::Result<()> {
    let state = State {
        options: options.clone(),
        single_access: Arc::new(Semaphore::new(1)),
    };

    let (thread_update_snd, thread_update_rec) = mpsc::unbounded_channel::<ThreadStatus>();

    let ws_route = warp::path::end()
        .and(with_state(state.clone()))
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(move |state: State, ws: warp::ws::Ws| {
            // And then our closure will be called when it completes...
            let thread_update_snd = thread_update_snd.clone();
            ws.on_upgrade(|websocket| async move {
                let use_timeout = state.options.idle_timeout.is_some();
                if use_timeout {
                    thread_update_snd
                        .send(ThreadStatus::Started)
                        .unwrap_or_else(|e| error!("Cannot send to thread update channel: {}", e));
                }

                handle_websocket(state, websocket)
                    .await
                    .unwrap_or_else(|e| error!("Error handling websocket: {:?}", e));

                if use_timeout {
                    thread_update_snd
                        .send(ThreadStatus::Finished)
                        .unwrap_or_else(|e| error!("Cannot send to thread update channel: {}", e));
                }
            })
        });

    let index = warp::path::end()
        .and(with_state(options.clone()))
        .map(redirect_to_websocket);

    // since websocket filter is more restrictive match on it first
    let routes = ws_route.or(index);

    let mut addrs = (options.host.as_str(), options.port)
        .to_socket_addrs()
        .with_context(|| format!("Invalid server address: {}:{}", options.host, options.port))?;
    let addr = addrs.next().unwrap();

    let server = warp::serve(routes);

    match options {
        Settings {
            idle_timeout: None,
            #[cfg(all(feature = "systemd", target_os = "linux"))]
                from_systemd: false,
            ..
        } => {
            info!("Listening on http://{}", addr);
            server.bind(addr).await;
        }
        Settings {
            idle_timeout: Some(timeout_sec),
            #[cfg(all(feature = "systemd", target_os = "linux"))]
                from_systemd: false,
            ..
        } => {
            info!("Listening on http://{}", addr);
            debug!("Idle timeout after {} secs", timeout_sec);
            let timeout_task =
                idle_timeout(time::Duration::from_secs(timeout_sec), thread_update_rec);
            let (_addr, serve_task) = server.bind_with_graceful_shutdown(addr, timeout_task);
            serve_task.await;
        }
        #[cfg(all(feature = "systemd", target_os = "linux"))]
        Settings {
            idle_timeout: None,
            from_systemd: true,
            ..
        } => {
            let listener_stream = systemd::try_get_socket()?;
            info!("Listening on systemd socket");
            server.serve_incoming(listener_stream).await;
        }
        #[cfg(all(feature = "systemd", target_os = "linux"))]
        Settings {
            idle_timeout: Some(timeout_sec),
            from_systemd: true,
            ..
        } => {
            let listener_stream = systemd::try_get_socket()?;
            info!("Listening on systemd socket");
            debug!("Idle timeout after {} secs", timeout_sec);
            let timeout_task =
                idle_timeout(time::Duration::from_secs(timeout_sec), thread_update_rec);
            server
                .serve_incoming_with_graceful_shutdown(listener_stream, timeout_task)
                .await;
        }
    }

    Ok(())
}

/// Send initial json redirect info for Ghost Text protocol
fn redirect_to_websocket(options: Settings) -> String {
    serde_json::to_string(&msg::RedirectToWebSocket {
        WebSocketPort: options.port.to_owned(),
        ProtocolVersion: 1,
    })
    .unwrap()
}

/// Communicate over a websocket, manage an intermediate file, spawn an editor, watch for changes
async fn handle_websocket(state: State, stream: WebSocket) -> anyhow::Result<()> {
    let (mut tx, mut rx) = stream.split();

    let init_message: Message = rx.next().await.expect("Need an initial edit message")?;

    debug!("First message: {:?}", init_message);
    let init_message: msg::GetTextFromComponent = if init_message.is_text() {
        serde_json::from_str(init_message.to_str().expect("Is a text msg"))
            .context("Couldn't parse initial websocket message")?
    } else {
        bail!("Initial websocket message not text")
    };

    // store client cursor changes and pass back and forth...
    let mut cursors = init_message.selections.clone();

    // create file
    let tempdir = TempDir::new("ghost-text")?;
    let file_path = file::get_new_path(tempdir.path(), &init_message)?;
    debug!("Creating file at: {:?}", file_path);
    file::replace_contents(&file_path, &init_message)?;

    // moar futures:
    // - pass off to editor, wait for exit
    // - add async file watcher to check for writes
    // - handle additional messages from websocket:
    //   - update file on change
    //   - ignore cursor updates
    //   - respond to pings?

    let rx = rx.fuse();
    let editor = lock_and_spawn(&state, &file_path, &init_message).fuse();
    let edits = file::watch_edits(&file_path).fuse();
    pin_mut!(rx, editor, edits);

    loop {
        futures::select! {
            e = editor => {
                if let Err(e) = e {
                error!("Error creating editor process: {}", e);
            }
                debug!("Editor closed!");
                break;
            },
            _edit = edits.select_next_some() => {
                debug!("File modified");
                send_current_file_contents(&mut tx, &file_path, &cursors).await?;
            },
            msg = rx.select_next_some() => match msg {
                Ok(msg) => {
                    if msg.is_text() {
                        let update_msg: msg::GetTextFromComponent = serde_json::from_str(msg.to_str().expect("Is a text msg")).context("Could not parse websocket message")?;
                        debug!("Received update msg");
                        cursors = update_msg.selections.to_owned();
                        file::replace_contents(&file_path, &update_msg)?;

                        // take next edit notification...
                        #[cfg(feature = "watch_changes")]
                            edits.select_next_some().await;

                        continue;
                    }
                    debug!("Received non-update msg: {:?}", msg);
                },
                Err(e) => error!("Websocket error: {}", e),
        },
        }
    }

    // return updated file text
    send_current_file_contents(&mut tx, &file_path, &cursors).await?;

    // close gracefully
    tx.close().await.context("closing websocket tx handle")?;

    drop(tempdir); // directory/file is deleted

    Ok(())
}

/// Acquire a global lock if configured and start the editor process
async fn lock_and_spawn(
    state: &State,
    file_path: &Path,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    let lock = if !state.options.multi {
        Some(state.single_access.acquire().await?)
    } else {
        None
    };

    editor::spawn_editor(&state.options, file_path, msg).await?;

    // the editor has either failed or finished, so allow another process to spawn
    drop(lock);

    Ok(())
}

async fn send_current_file_contents(
    stream: &mut WebSocketTx,
    file_path: &Path,
    cursors: &[msg::RangeInText],
) -> anyhow::Result<()> {
    let text = file::get_current_contents(file_path).await?;

    stream
        .send(Message::text(serde_json::to_string(
            &msg::SetTextInComponent {
                text: text.as_ref(),
                selections: cursors.to_owned(),
            },
        )?))
        .await?;

    Ok(())
}

enum ThreadStatus {
    Started,
    Finished,
}

async fn idle_timeout(
    duration: time::Duration,
    status_updater: mpsc::UnboundedReceiver<ThreadStatus>,
) {
    let mut alive_count: usize = 0;
    let mut updater = UnboundedReceiverStream::new(status_updater);

    loop {
        let update = if alive_count == 0 {
            time::timeout(duration, updater.next()).await
        } else {
            Ok(updater.next().await)
        };

        match update {
            Err(_) /* time::error::Elapsed, compiler doesn't like writing it inside the match arm */ => {
                info!("Stopping after idle timeout of {} secs", duration.as_secs());
                break;
            }
            Ok(None) => {
                error!("All thread status sending handles dropped; stopping");
                break;
            }
            Ok(Some(ThreadStatus::Started)) => {
                alive_count += 1;
            }
            Ok(Some(ThreadStatus::Finished)) => {
                alive_count -= 1;
            }
        }
    }
}
