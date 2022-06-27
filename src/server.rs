use std::{
    env,
    fs::File,
    io::Write,
    net::ToSocketAddrs,
    os::unix,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context};
use inotify::{Inotify, WatchMask};
use tokio::{
    fs, io, net,
    process::Command,
    sync::{mpsc, Semaphore},
    time,
};
use tokio_stream::wrappers::{UnboundedReceiverStream, UnixListenerStream};

use futures::{pin_mut, stream::SplitSink, FutureExt, SinkExt, Stream, StreamExt};
use tempdir::TempDir;
use warp::{
    ws::{Message, WebSocket},
    Filter,
};

use crate::{
    ws_messages::{self as msg},
    Options,
};

type WebSocketTx = SplitSink<WebSocket, Message>;

#[derive(Debug, Clone)]
struct State {
    options: Options,
    single_access: Arc<Semaphore>,
}

fn with_state<S: Clone + Send>(
    state: S,
) -> impl Filter<Extract = (S,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

pub async fn run(options: Options) -> anyhow::Result<()> {
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

    let requested_addr = format!("{}:{}", options.host, options.port);

    let mut addrs = (options.host, options.port)
        .to_socket_addrs()
        .with_context(|| format!("Invalid server address: {}", requested_addr))?;
    let addr = addrs.next().unwrap();

    let server = warp::serve(routes);

    if let Some(timeout_sec) = options.idle_timeout {
        debug!("Idle timeout after {} secs", timeout_sec);
        let timeout_task = idle_timeout(time::Duration::from_secs(timeout_sec), thread_update_rec);

        if options.from_systemd {
            let listener_stream = systemd_socket()?;
            info!("Listening on systemd socket");
            server
                .serve_incoming_with_graceful_shutdown(listener_stream, timeout_task)
                .await;
        } else {
            info!("Listening on http://{}", addr);
            let (_addr, serve_task) = server.bind_with_graceful_shutdown(addr, timeout_task);
            serve_task.await;
        }
    } else if options.from_systemd {
        let listener_stream = systemd_socket()?;
        info!("Listening on systemd socket");
        server.serve_incoming(listener_stream).await;
    } else {
        info!("Listening on http://{}", addr);
        server.bind(addr).await;
    }

    Ok(())
}

/// Send initial json redirect info for Ghost Text protocol
fn redirect_to_websocket(options: Options) -> String {
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
    let file_path = get_new_path(tempdir.path(), &init_message)?;
    debug!("Creating file at: {:?}", file_path);
    init_file(&file_path, &init_message)?;

    // moar futures:
    // - pass off to editor, wait for exit
    // - add async file watcher to check for writes
    // - handle additional messages from websocket:
    //   - update file on change
    //   - ignore cursor updates
    //   - respond to pings?

    let rx = rx.fuse();
    let editor = lock_and_spawn(&state, &file_path, &init_message).fuse();
    let edits = watch_file_edits(&file_path)?.fuse();
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
            event = edits.select_next_some() => match event {
                Ok(_) => {
                    debug!("File modified");
                    send_current_file_contents(&mut tx, &file_path, &cursors).await?;
                },
                Err(e) => error!("inotify error: {}", e)
            },
            msg = rx.select_next_some() => match msg {
                Ok(msg) => {
                    if msg.is_text() {
                        let update_msg: msg::GetTextFromComponent = serde_json::from_str(msg.to_str().expect("Is a text msg")).context("Could not parse websocket message")?;
                        debug!("Received update msg");
                        cursors = update_msg.selections.to_owned();
                        init_file(&file_path, &update_msg)?;
                        // take next edit notification...
                        if let Err(e) = edits.select_next_some().await {
                            error!("inotify error after writing: {}", e);
                        }
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

    drop(tempdir); // delete directory/file
    Ok(())
}

fn process_title(title: &str) -> String {
    const BAD_CHARS: &[char] = &[' ', '/', '\\', '\r', '\n', '\t'];

    let mut title = title;

    let file_name = if title.is_empty() {
        String::from("buffer")
    } else {
        if title.len() > 16 {
            if let Some((i, _c)) = title.char_indices().nth(16) {
                title = &title[..i];
            }
        }
        title.replace(BAD_CHARS, "-")
    } + ".txt";

    file_name
}

fn get_new_path(dir: &Path, msg: &msg::GetTextFromComponent) -> io::Result<PathBuf> {
    let file_name = process_title(&msg.title);
    let file_path = dir.join(file_name);
    Ok(file_path)
}

fn init_file(path: &PathBuf, msg: &msg::GetTextFromComponent) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(msg.text.as_bytes())?;
    if !msg.text.ends_with('\n') {
        file.write_all(&[b'\n'])?;
    }
    Ok(())
}

/// Convert the browser's 0-based UTF-16 offset to 1-based UTF-8 line/col cursor coordinates
fn utf16_offset_to_utf8_line_col(offset: usize, text: &str) -> (usize, usize) {
    // re-encode text as UTF-16
    let chars = text.encode_utf16();

    let mut line = 1;
    let mut utf16_col = 1;

    let mut last_line = Vec::new();

    for (i, c) in chars.enumerate() {
        if i < offset {
            if c == '\n' as u16 {
                line += 1;
                utf16_col = 1;
                last_line.clear();
            } else {
                utf16_col += 1;
                last_line.push(c);
            }
        } else {
            // continue reading line to buffer
            if c == '\n' as u16 {
                break;
            }
            last_line.push(c);
        }
    }

    trace!("last line: {:?}", last_line);

    // don't need to compute anything else if at beginning of last line
    if utf16_col == 1 {
        trace!("cursor at beginning of line");
        let col = 1;
        trace!("col: {}", col);
        return (line, col);
    }

    // substring of last line to cursor
    // includes all characters BEFORE browser-style cursor (pipe)
    // does not include character highlighted by terminal-style cursor (block)
    let to_cursor = &last_line[..utf16_col - 1];
    trace!("to_cursor: {:?}", to_cursor);

    // if substring to cursor is valid, it's length is the byte before the one we want the cursor to land on
    if let Ok(to_cursor) = String::from_utf16(to_cursor) {
        trace!("using valid cursor position");
        let col = to_cursor.len() + 1;
        trace!("col: {}", col);
        return (line, col);
    }

    // if the substring is invalid, we're in the middle of a bad byte
    // try to backtrack to last good byte
    trace!("cursor on bad byte");

    let decode_results: Vec<_> = std::char::decode_utf16(to_cursor.to_owned())
        .map(|r| r.map_err(|e| e.unpaired_surrogate()))
        .collect();
    trace!("decode results: {:?}", decode_results);

    // trim trailing bad bytes
    let trailing_err = decode_results
        .iter()
        .rposition(|r| r.is_ok())
        .map(|i| i + 1) // convert to 1-based
        .unwrap_or(decode_results.len());
    let decode_results = &decode_results[..trailing_err];
    trace!("cleaned decode results: {:?}", decode_results);

    let col = decode_results
        .iter()
        .map(|r| match r {
            Ok(c) => c.len_utf8(),
            // FIXME: see if this is really right for UTF16...
            Err(w) if *w < 127 => 1,
            Err(_) => 2,
        })
        .sum::<usize>() + 1;

    trace!("col: {}", col);

    (line, col)
}

/// If configured, acquire a global lock before starting the editor process
async fn lock_and_spawn(
    state: &State,
    file_path: &PathBuf,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    let lock = if !state.options.multi {
        Some(state.single_access.acquire().await?)
    } else {
        None
    };

    spawn_editor(&state.options, file_path, msg).await?;

    // the editor has either failed or finished, so allow another process to spawn
    drop(lock);

    Ok(())
}

fn replace_in_place(source: &mut String, pattern: &str, replacement: &str) -> bool {
    let start = match source.find(pattern) {
        None => return false,
        Some(s) => s,
    };

    source.replace_range(start..(start + pattern.len()), replacement);
    return true;
}

/// Returns on process exit
async fn spawn_editor(
    options: &Options,
    file_path: &PathBuf,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    info!("New session from: {:?}", msg.title);

    // TODO: fix this up
    let file_path = file_path.to_str().expect("file path should be good");

    let (line, col) = msg
        .selections
        .get(0)
        .map(|s| utf16_offset_to_utf8_line_col(s.start, &msg.text))
        .unwrap_or((1, 1));

    let mut pieces =
        shell_words::split(&options.editor).context("Could not parse editor command")?;

    let mut did_replace_file = false;

    for s in pieces.iter_mut().skip(1) {
        replace_in_place(s, "%l", &line.to_string());
        replace_in_place(s, "%c", &col.to_string());
        if replace_in_place(s, "%f", &file_path) {
            did_replace_file = true;
        }
    }

    if pieces.is_empty() {
        bail!("Empty editor command");
    }

    if !did_replace_file {
        pieces.push(file_path.to_string());
    }

    let program = &pieces[0];

    let args = &pieces[1..];

    debug!("Opening editor {:?}", pieces);

    let exit_status = Command::new(program)
        .args(args)
        .env("GHOST_TEXT_URL", &msg.url)
        .env("GHOST_TEXT_TITLE", &msg.title)
        .spawn()?
        .wait()
        .await?;

    if !exit_status.success() {
        error!("Editor process exited with status: {}", exit_status);
    }

    Ok(())
}

/// Returns a stream of update events for the provided file
fn watch_file_edits(path: &PathBuf) -> io::Result<impl Stream<Item = Result<(), io::Error>>> {
    let mut watcher = Inotify::init()?;
    watcher.add_watch(path, WatchMask::MODIFY)?;
    let buffer = [0u8; 32];
    let stream = watcher.event_stream(buffer)?.map(|op| {
        op.map(|event| {
            trace!("inotify event: {:?}", event);
        })
    });
    Ok(stream)
}

async fn send_current_file_contents(
    stream: &mut WebSocketTx,
    file_path: &PathBuf,
    cursors: &[msg::RangeInText],
) -> anyhow::Result<()> {
    let text = current_file_contents(file_path).await?;

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

async fn current_file_contents(file_path: &PathBuf) -> io::Result<String> {
    let mut text = fs::read_to_string(file_path).await?;

    if text.ends_with('\n') {
        text.pop();
    }

    Ok(text)
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

/// Try to get a listener socket passed by systemd.
///
/// This function should only be called once.
fn systemd_socket() -> anyhow::Result<UnixListenerStream> {
    const START_FD: usize = 3; // SD_LISTEN_FDS_START, see sd_listen_fds(3)
    const LISTEN_PID: &str = "LISTEN_PID";
    const LISTEN_FD_NAMES: &str = "LISTEN_FD_NAMES";
    const LISTEN_FDS: &str = "LISTEN_FDS";

    debug!("LISTEN_PID={:?}", env::var_os(LISTEN_PID));
    debug!("LISTEN_FD_NAMES={:?}", env::var_os(LISTEN_FD_NAMES));
    debug!("LISTEN_FDS={:?}", env::var_os(LISTEN_FDS));

    let num_fds: usize = env::var(LISTEN_FDS)?.parse()?;
    if num_fds > 1 {
        anyhow::bail!("More than one systemd socket file descriptor present");
    } else if num_fds == 0 {
        anyhow::bail!("No systemd socket file descriptors present");
    }

    // only one socket
    let fd = START_FD as unix::io::RawFd;

    // turn fd into std UnixListener
    // Safety: only called once, environment variables are removed to prevent reuse
    use unix::io::FromRawFd;
    let listener = unsafe { unix::net::UnixListener::from_raw_fd(fd) };
    listener.set_nonblocking(true)?;

    // convert to tokio UnixListenerStream
    let listener = net::UnixListener::from_std(listener)?;
    let listener_stream = UnixListenerStream::new(listener);

    // Remove environment variables to prevent reuse
    env::remove_var(LISTEN_PID);
    env::remove_var(LISTEN_FD_NAMES);
    env::remove_var(LISTEN_FDS);

    Ok(listener_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("asdf hjkl", 0 => (1, 1)   ; "at text beginning")]
    //           ^here
    #[test_case("asdf hjkl", 10 => (1, 10) ; "at text end")]
    //                    ^here
    #[test_case("asdf hjkl", 4 => (1, 5)   ; "in text")]
    //               ^here
    #[test_case("asdf hjkl", 12 => (1, 10) ; "past end of text")]
    //                      ^here
    #[test_case("asdf\nhjkl\nzxcv", 7 => (2, 3)  ; "on another line")]
    //                   ^here
    #[test_case("asdf 🇺🇸", 5 => (1, 6)     ; "at beginning of surrogate pair")]
    //                ^here
    #[test_case("asdf 🇺🇸", 9 => (1, 14)    ; "at end of surrogate pair")]
    //                  ^here
    #[test_case("asdf 🇺🇸", 6 => (1, 6)     ; "in middle of surrogate pair")]
    //                ^here
    #[test_log::test]
    fn offset_conversions(text: &str, offset: usize) -> (usize, usize) {
        utf16_offset_to_utf8_line_col(offset, text)
    }
}
