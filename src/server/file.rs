use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

use futures::Stream;
use inotify::{Inotify, WatchMask};
use log::trace;
use tokio::fs;
use tokio_stream::StreamExt;

use super::msg;

pub fn get_new_path(dir: &Path, msg: &msg::GetTextFromComponent) -> io::Result<PathBuf> {
    let file_name = process_title(&msg.title);
    let file_path = dir.join(file_name);
    Ok(file_path)
}

pub fn replace_contents(path: &PathBuf, msg: &msg::GetTextFromComponent) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(msg.text.as_bytes())?;
    if !msg.text.ends_with('\n') {
        file.write_all(&[b'\n'])?;
    }
    Ok(())
}

/// Returns a stream of update events for the provided file
pub fn watch_edits(path: &PathBuf) -> io::Result<impl Stream<Item = Result<(), io::Error>>> {
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

pub async fn get_current_contents(file_path: &PathBuf) -> io::Result<String> {
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
