use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{ready, stream::Fuse, Future, Stream, StreamExt};
use pin_project::pin_project;
use tokio::time::{sleep_until, Duration, Instant, Sleep};

pub trait MyStreamExt: Stream + Sized {
    /// Returns the latest item after `wait` has elapsed with no new items, dropping intermediate ones.
    ///
    /// A `wait` of zero returns all items in the original stream with no delay.
    ///
    /// Returns last item immediately if stream is closed.
    fn debounce(self, wait: Duration) -> Debounce<Self> {
        Debounce::new(self, wait)
    }
}

impl<S: Stream + Sized> MyStreamExt for S {}

#[must_use = "streams do nothing unless polled"]
#[derive(Debug)]
#[pin_project]
pub struct Debounce<S: Stream> {
    #[pin]
    stream: Fuse<S>,
    #[pin]
    deadline: Sleep,
    last: Option<S::Item>,
    duration: Duration,
}

impl<S: Stream> Debounce<S> {
    fn new(stream: S, duration: Duration) -> Self {
        let next = Instant::now() + duration;
        let deadline = sleep_until(next);

        Self {
            stream: stream.fuse(),
            last: None,
            deadline,
            duration,
        }
    }
}

impl<S: Stream> Stream for Debounce<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut me = self.project();

        // poll stream until we get Pending in order to store waker
        while let Poll::Ready(v) = me.stream.as_mut().poll_next(cx) {
            if v.is_none() {
                // ensure last item gets out if kept
                // stream is fused, so it can be polled while empty multiple times
                return Poll::Ready(me.last.take());
            }

            if me.duration.is_zero() {
                return Poll::Ready(v);
            }

            // store for later
            *me.last = v;

            let next = Instant::now() + *me.duration;
            me.deadline.as_mut().reset(next);
        }

        // if we have an item, return if timer is up
        if me.last.is_some() {
            ready!(me.deadline.poll(cx));
            return Poll::Ready(me.last.take());
        }

        Poll::Pending
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio_stream::{self as stream, StreamExt};

    #[tokio::test]
    async fn test_debounce() {
        let s = stream::iter(1..=2)
            .chain(stream::iter(3..=5).throttle(Duration::from_millis(150)))
            .debounce(Duration::from_millis(100));
        tokio::pin!(s);
        assert_eq!(vec![3, 4, 5], s.collect::<Vec<_>>().await);
    }

    #[tokio::test]
    async fn test_debounce_zero_returns_all() {
        let s = stream::iter(1..=5).debounce(Duration::default());
        tokio::pin!(s);
        assert_eq!(vec![1, 2, 3, 4, 5], s.collect::<Vec<_>>().await);
    }

    #[tokio::test]
    async fn test_debounce_returns_last() {
        let s = stream::iter(1..=5).debounce(Duration::from_millis(50));
        tokio::pin!(s);
        assert_eq!(vec![5], s.collect::<Vec<_>>().await);
    }
}
