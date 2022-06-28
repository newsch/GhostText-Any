use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use futures::Stream;
use log::{debug, error, trace};
use tokio::{fs, sync::mpsc};

use super::msg;

pub fn get_new_path(dir: &Path, msg: &msg::GetTextFromComponent) -> io::Result<PathBuf> {
    let file_name = process_title(&msg.title);
    let file_path = dir.join(file_name);
    Ok(file_path)
}

pub fn replace_contents(path: &Path, msg: &msg::GetTextFromComponent) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(msg.text.as_bytes())?;
    if !msg.text.ends_with('\n') {
        file.write_all(&[b'\n'])?;
    }
    Ok(())
}

/// Returns a stream of update events for the provided file
pub fn watch_edits(path: &Path) -> impl Stream<Item = ()> {
    let path = path.to_owned();

    let (tx, rx) = mpsc::channel(8);
    let _task = tokio::task::spawn_blocking(move || {
        if let Err(e) = notify_thread(&path, tx) {
            error!("Error on notify_thread: {e}");
        }
    });

    tokio_stream::wrappers::ReceiverStream::new(rx)
}

/// Blocking loop to read from non-async notify
fn notify_thread(path: &Path, sender: mpsc::Sender<()>) -> anyhow::Result<()> {
    use notify::{Op, RawEvent, RecursiveMode, Watcher};

    let (tx, rx) = std::sync::mpsc::channel();

    let mut watcher = notify::raw_watcher(tx).context("creating notify watcher")?;
    watcher.watch(path, RecursiveMode::NonRecursive)?;

    loop {
        let event = rx.recv().context("recv from notify watcher")?;
        trace!("New notify event for {path:?}: {event:?}");
        if let RawEvent { op: Ok(op), .. } = event {
            if op.contains(Op::WRITE) {
                if let Err(e) = sender.blocking_send(()) {
                    debug!("file watcher receiver closed, stopping: {e}");
                    break;
                }
            }
        }
    }

    Ok(())
}

pub async fn get_current_contents(file_path: &Path) -> io::Result<String> {
    let mut text = fs::read_to_string(file_path).await?;

    if text.ends_with('\n') {
        text.pop();
    }

    Ok(text)
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
