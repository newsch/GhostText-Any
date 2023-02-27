use std::path::Path;

use futures::{Stream, StreamExt};
use tokio::sync::mpsc;

/// Returns a stream of update events for the provided file
pub fn watch_edits(path: &Path) -> anyhow::Result<impl Stream<Item = ()>> {
    use notify::Watcher;

    let (mut watcher, rx) = async_watcher()?;

    watcher.watch(path.as_ref(), notify::RecursiveMode::NonRecursive)?;

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Ok(NotifyWatcherStream {
        _watcher: watcher,
        stream,
    })
}

/// Wrapper to keep watcher alive with event stream handle
struct NotifyWatcherStream {
    _watcher: notify::RecommendedWatcher,
    stream: tokio_stream::wrappers::ReceiverStream<()>,
}

impl Stream for NotifyWatcherStream {
    type Item = ();

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.stream.poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

fn async_watcher() -> notify::Result<(notify::RecommendedWatcher, mpsc::Receiver<()>)> {
    use notify::{Event, EventKind};

    let (tx, rx) = mpsc::channel(1);
    let handle = tokio::runtime::Handle::current();

    let watcher = notify::recommended_watcher(move |res| match res {
        Err(e) => debug!("Notify error: {e}"),
        Ok(event) => {
            trace!("New notify event: {event:?}");
            if let Event {
                kind: EventKind::Modify(_),
                ..
            } = event
            {
                handle.block_on(async {
                    tx.send(()).await.unwrap();
                })
            }
        }
    })?;

    Ok((watcher, rx))
}
