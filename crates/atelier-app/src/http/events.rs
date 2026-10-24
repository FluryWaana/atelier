use axum::response::{
    IntoResponse, Response,
    sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};
use tokio::sync::{broadcast, watch};

#[derive(Deserialize)]
pub(super) struct Credentials {
    pub session: Option<String>,
}

fn notifications(
    changes: broadcast::Receiver<()>,
    closing: watch::Receiver<bool>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(
        (changes, closing, true),
        |(mut changes, mut closing, first)| async move {
            if *closing.borrow() {
                return None;
            }
            let event = if first {
                Event::default()
                    .retry(Duration::from_secs(1))
                    .data("connected")
            } else {
                tokio::select! {
                    biased;
                    _ = closing.changed() => return None,
                    change = changes.recv() => { change.ok()?; }
                }
                // Coalesce committed mutations; a lagging client reconnects and reloads state.
                tokio::select! {
                    biased;
                    _ = closing.changed() => return None,
                    _ = tokio::time::sleep(Duration::from_millis(75)) => {}
                }
                loop {
                    match changes.try_recv() {
                        Ok(()) => {}
                        Err(broadcast::error::TryRecvError::Empty) => break,
                        Err(_) => return None,
                    }
                }
                Event::default().data("changed")
            };
            Some((Ok(event), (changes, closing, false)))
        },
    )
}

pub(super) fn response(
    changes: broadcast::Receiver<()>,
    closing: watch::Receiver<bool>,
) -> Response {
    (
        [("x-accel-buffering", "no"), ("connection", "keep-alive")],
        Sse::new(notifications(changes, closing)).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("heartbeat"),
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn slow_or_closing_subscribers_end_and_release_their_subscription() {
        let (changes, _) = broadcast::channel(2);
        let (closing, _) = watch::channel(false);
        {
            let stream = notifications(changes.subscribe(), closing.subscribe());
            let mut stream = std::pin::pin!(stream);
            assert!(stream.next().await.is_some());
            for _ in 0..3 {
                changes.send(()).unwrap();
            }
            assert!(stream.next().await.is_none());
        }
        assert_eq!(changes.receiver_count(), 0);
        {
            let stream = notifications(changes.subscribe(), closing.subscribe());
            let mut stream = std::pin::pin!(stream);
            assert!(stream.next().await.is_some());
            closing.send(true).unwrap();
            assert!(stream.next().await.is_none());
        }
        assert_eq!(changes.receiver_count(), 0);
        let stream = notifications(changes.subscribe(), closing.subscribe());
        let mut stream = std::pin::pin!(stream);
        assert!(stream.next().await.is_none());
    }
}
