use std::{net::ToSocketAddrs, path::Path, sync::Arc};

use anyhow::{bail, Context};
use tokio::{
    sync::{mpsc, Semaphore},
    time::{self, timeout, Duration},
};
use tokio_stream::wrappers::UnboundedReceiverStream;

use futures::FutureExt;
use futures::{pin_mut, stream::SplitSink, SinkExt, StreamExt};
use url::Url;
use warp::{
    http::HeaderValue,
    reject::reject,
    ws::{Message, WebSocket},
    Filter,
};

mod editor;
mod file;
use file::{watch_edits, LocalFile};
mod msg;
mod text;
#[cfg(feature = "watch_changes")]
mod watch_changes;

use crate::debounce::MyStreamExt;
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

/// Ensures the request Origin header is set to an extension uri.
///
/// If a Websocket request is sent by a browser, the origin will be set to:
/// - `null`
/// - the url of the initiating webpage
/// - some form of `*-extension://*` if initiated by an extension
///
/// Restricting it to extensions prevents random websites from trying to exfiltrate or exploit.
/// See: <https://christian-schneider.net/CrossSiteWebSocketHijacking.html>.
fn is_extension_origin() -> impl Filter<Extract = (), Error = warp::reject::Rejection> + Copy {
    warp::header::value("origin")
        .and_then(|origin: HeaderValue| async move {
            // Verify websocket is from extension context
            let origin = origin.to_str().map_err(|e| {
                warn!("Rejecting request from non-string origin: {origin:?}: {e}");
                reject()
            })?;
            let origin = Url::parse(origin).map_err(|e| {
                warn!("Rejecting request from unparseable origin: {origin:?}: {e}");
                reject()
            })?;

            if !origin.scheme().ends_with("extension") {
                warn!("Rejecting request from non-extension origin: {origin:?}");
                return Err(reject());
            }

            Ok(())
        })
        .untuple_one()
}

pub async fn run(options: Settings) -> anyhow::Result<()> {
    let state = State {
        options: options.clone(),
        single_access: Arc::new(Semaphore::new(1)),
    };

    let (thread_update_snd, thread_update_rec) = mpsc::unbounded_channel::<ThreadStatus>();

    let ws_route = warp::path::end()
        .and(is_extension_origin())
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
    let routes = ws_route
        .or(index)
        .with(warp::log::log("gtany::server::request"));

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
            let listener_stream = super::systemd::try_get_socket()?;
            info!("Listening on systemd socket");
            server.serve_incoming(listener_stream).await;
        }
        #[cfg(all(feature = "systemd", target_os = "linux"))]
        Settings {
            idle_timeout: Some(timeout_sec),
            from_systemd: true,
            ..
        } => {
            let listener_stream = super::systemd::try_get_socket()?;
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
    let mut file = LocalFile::create(&init_message).await?;
    let file_path = file.as_ref().to_owned();

    // moar futures:
    // - pass off to editor, wait for exit
    // - add async file watcher to check for writes
    // - handle additional messages from websocket:
    //   - update file on change
    //   - ignore cursor updates
    //   - respond to pings?

    const EDIT_DELAY_MS: u64 = 200;

    let rx = {
        let msg_delay = Duration::from_millis(state.options.delay);

        // async closures not stable
        async fn ws_error(m: Result<Message, warp::Error>) -> Option<Message> {
            m.map(|m| {
                trace!("Received websocket msg: {:?}", m);
                m
            })
            .map_err(|e| error!("Websocket error: {}", e))
            .ok()
        }

        rx.filter_map(ws_error)
            .debounce(msg_delay)
            .inspect(|m| debug!("Debounced websocket msg: {m:?}"))
            .fuse()
    };

    let editor = lock_and_spawn(&state, &file_path, &init_message).fuse();
    let edits = watch_edits(&file_path)
        .context("watch_edits")?
        .debounce(Duration::from_millis(EDIT_DELAY_MS))
        .inspect(|e| debug!("Debounced notify event: {e:?}"))
        .fuse();
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
                send_current_file_contents(&mut tx, &mut file, &cursors).await?;
            },
            msg = rx.select_next_some() => {
                if !msg.is_text() {
                    error!("Received non-update msg: {:?}", msg);
                    continue;
                }
                let update_msg: msg::GetTextFromComponent = serde_json::from_str(
                    msg.to_str().expect("Is a text msg")).context("Could not parse websocket message")?;
                debug!("Handling update msg");
                cursors = update_msg.selections.to_owned();
                let did_write = file.maybe_update(&update_msg).await?;

                #[cfg(feature = "watch_changes")]
                if did_write {
                    debug!("Ignoring next edit notification");
                    match timeout(Duration::from_millis(EDIT_DELAY_MS / 2 * 3), edits.select_next_some()).await {
                        Ok(_) => debug!("Got next edit notification"),
                        Err(_) => warn!("Timed out waiting for next edit notification"),
                    }
                }
            },
        }
    }

    // return updated file text
    send_current_file_contents(&mut tx, &mut file, &cursors).await?;

    // close gracefully
    tx.close().await.context("closing websocket tx handle")?;

    Ok(())
}

/// Acquire a global lock if configured and start the editor process
async fn lock_and_spawn(
    state: &State,
    file_path: impl AsRef<Path>,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    let lock = if !state.options.multi {
        Some(state.single_access.acquire().await?)
    } else {
        None
    };

    editor::spawn_editor(&state.options, file_path.as_ref(), msg).await?;

    // the editor has either failed or finished, so allow another process to spawn
    drop(lock);

    Ok(())
}

async fn send_current_file_contents(
    stream: &mut WebSocketTx,
    file: &mut file::LocalFile,
    cursors: &[msg::RangeInText],
) -> anyhow::Result<()> {
    let text = file.get_current_contents().await?;

    debug!("Sending update msg");
    stream
        .send(Message::text(serde_json::to_string(
            &msg::SetTextInComponent {
                text: &text,
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
