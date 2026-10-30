use axum::response::{
    IntoResponse, Response,
    sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::{self, Stream};
use std::{convert::Infallible, time::Duration};
use tokio::sync::{broadcast, watch};

pub(super) fn authenticated(request: &axum::extract::Request, port: u16, expected: &str) -> bool {
    let prefix = format!("atelier-events-{port}=");
    request
        .headers()
        .get_all(axum::http::header::COOKIE)
        .iter()
        .any(|header| {
            header.to_str().ok().is_some_and(|cookies| {
                cookies.split(';').any(|cookie| {
                    cookie.trim().strip_prefix(&prefix).is_some_and(|token| {
                        crate::infrastructure::session::same_secret(token, expected)
                    })
                })
            })
        })
}

pub(super) async fn session(
    axum::extract::State(state): axum::extract::State<super::Shared>,
) -> Response {
    // This independent credential grants only the notification stream, never API access.
    (
        [(
            axum::http::header::SET_COOKIE,
            format!(
                "atelier-events-{}={}; HttpOnly; SameSite=Strict; Path=/api/events",
                state.port, state.event_token
            ),
        )],
        axum::Json(serde_json::json!({"ok": true})),
    )
        .into_response()
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

    #[test]
    fn stream_cookies_are_scoped_to_one_listener_and_compare_the_whole_secret() {
        for (cookies, accepted) in [
            ("unrelated=x; atelier-events-4328=synthetic", true),
            ("atelier-events-4329=synthetic", false),
            ("atelier-events-4328=synthetic-extra", false),
            ("atelier-events-4328=wrong", false),
            ("prefix-atelier-events-4328=synthetic", false),
            ("", false),
        ] {
            let request = axum::http::Request::builder()
                .header("cookie", cookies)
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(authenticated(&request, 4328, "synthetic"), accepted);
        }
    }

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
