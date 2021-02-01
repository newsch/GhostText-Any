use std::{
    error::Error,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use inotify::{Inotify, WatchMask};
use tokio::{fs, io, process::Command};

use futures::{pin_mut, stream::SplitSink, FutureExt, SinkExt, Stream, StreamExt};
use tempdir::TempDir;
use warp::{
    ws::{Message, WebSocket},
    Filter,
};

use crate::{ws_messages as msg, Options};

type State = Options;
type WebSocketTx = SplitSink<WebSocket, Message>;

fn with_state(
    state: State,
) -> impl Filter<Extract = (State,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

pub async fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let ws_route = warp::path::end()
        .and(with_state(options.clone()))
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(|state, ws: warp::ws::Ws| {
            // And then our closure will be called when it completes...
            ws.on_upgrade(|websocket| async {
                handle_websocket(state, websocket)
                    .await
                    .unwrap_or_else(|e| error!("Error handling websocket: {}", e))
            })
        });

    let index = warp::path::end()
        .and(with_state(options.clone()))
        .map(redirect_to_websocket);

    // since websocket filter is more restrictive match on it first
    let routes = ws_route.or(index);

    warp::serve(routes)
        .run(([127, 0, 0, 1], options.port))
        .await;

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
async fn handle_websocket(options: State, stream: WebSocket) -> Result<(), Box<dyn Error>> {
    let (mut tx, mut rx) = stream.split();

    let init_message: Message = rx.next().await.expect("Need an initial edit message")?;

    debug!("First message: {:?}", init_message);
    let init_message: msg::GetTextFromComponent = if init_message.is_text() {
        serde_json::from_str(init_message.to_str().unwrap()).unwrap()
    } else {
        panic!("Expect first msg to be text")
    };

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
    let editor = spawn_editor(&options, &file_path, &init_message).fuse();
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
                    send_current_file_contents(&mut tx, &file_path).await?;
                },
                Err(e) => error!("inotify error: {}", e)
            },
            ws = rx.select_next_some() => info!("New websocket msg: {:?}", ws),
        }
    }

    // return updated file text
    send_current_file_contents(&mut tx, &file_path).await?;

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
        file.write(&['\n' as u8])?;
    }
    Ok(())
}

/// Returns on process exit
async fn spawn_editor(
    options: &Options,
    file_path: &PathBuf,
    msg: &msg::GetTextFromComponent,
) -> Result<(), io::Error> {
    let pieces = shell_words::split(&options.editor).unwrap();

    let program = pieces.get(0).ok_or("Empty editor").unwrap();
    let args = &pieces[1..];

    debug!("Opening editor {:?} for {:?}", pieces, file_path);
    let exit_status = Command::new(program)
        .args(args)
        .arg(file_path)
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

/// Returns a stream of update events
fn watch_file_edits(path: &PathBuf) -> io::Result<impl Stream<Item = Result<(), io::Error>>> {
    let mut watcher = Inotify::init()?;
    watcher.add_watch(path, WatchMask::MODIFY)?;
    let buffer = [0u8; 32];
    let stream = watcher.event_stream(buffer)?.map(|op| {
        op.map(|event| {
            trace!("inotify event: {:?}", event);
            ()
        })
    });
    Ok(stream)
}

async fn send_current_file_contents(
    stream: &mut WebSocketTx,
    file_path: &PathBuf,
) -> Result<(), warp::Error> {
    let text = current_file_contents(file_path).await.unwrap();

    stream
        .send(Message::text(
            serde_json::to_string(&msg::SetTextInComponent {
                text: text.as_ref(),
            })
            .unwrap(),
        ))
        .await?;

    Ok(())
}

async fn current_file_contents(file_path: &PathBuf) -> io::Result<String> {
    let mut text = fs::read_to_string(file_path).await?;

    if text.ends_with("\n") {
        text.pop();
    }

    Ok(text)
}
