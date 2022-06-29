use std::path::Path;

use anyhow::Context;
use futures::Stream;
use log::{debug, error, trace};
use tokio::sync::mpsc;

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
