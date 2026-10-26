use super::failure;
use axum::{http::StatusCode, response::Response};
use std::{future::Future, sync::Arc};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

enum Reservation {
    Ordinary { _guard: OwnedRwLockReadGuard<()> },
    External { _guard: OwnedRwLockWriteGuard<()> },
}

pub(super) async fn run(
    gate: Arc<RwLock<()>>,
    exclusive: bool,
    operation: impl Future<Output = Response> + Send + 'static,
) -> Response {
    let reservation = if exclusive {
        gate.try_write_owned()
            .map(|guard| Reservation::External { _guard: guard })
    } else {
        gate.try_read_owned()
            .map(|guard| Reservation::Ordinary { _guard: guard })
    };
    let Ok(guard) = reservation else {
        return failure(
            StatusCode::CONFLICT,
            "Une modification est en cours. Réessaie dans un instant.",
        );
    };
    detached(async move {
        let _guard = guard;
        operation.await
    })
    .await
}

pub(super) fn is_cancellation(path: &str) -> bool {
    path == "/api/prototype/stop"
        || path == "/api/shutdown"
        || path == "/api/updates/recover"
        || path == "/api/queue/pause"
        || path.strip_prefix("/api/time/").is_some_and(|tail| {
            tail.split_once('/')
                .is_some_and(|(id, action)| !id.is_empty() && action == "stop")
        })
        || path.strip_prefix("/api/runs/").is_some_and(|tail| {
            tail.split_once('/')
                .is_some_and(|(id, action)| !id.is_empty() && action == "stop")
        })
        || path.strip_prefix("/api/cards/").is_some_and(|tail| {
            tail.split_once('/').is_some_and(|(id, action)| {
                !id.is_empty() && matches!(action, "services/stop" | "automation/stop")
            })
        })
}

// Accepted writes and cancellations must finish even when the browser disconnects.
pub(super) async fn detached(
    operation: impl Future<Output = Response> + Send + 'static,
) -> Response {
    match tokio::spawn(operation).await {
        Ok(response) => response,
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "La modification a été interrompue. Actualise l’état avant de réessayer.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn only_known_stop_routes_bypass_the_mutation_gate() {
        for path in [
            "/api/prototype/stop",
            "/api/shutdown",
            "/api/updates/recover",
            "/api/queue/pause",
            "/api/time/entry/stop",
            "/api/runs/run/stop",
            "/api/cards/card/services/stop",
            "/api/cards/card/automation/stop",
        ] {
            assert!(is_cancellation(path));
        }
        for path in [
            "/api/runs//stop",
            "/api/cards/card/services/restart",
            "/api/runs/run/stop/extra",
            "/api/cards/card/jira/transition",
            "/api/cards/card/stop",
            "/api/backups/stop",
            "/api/shutdown/extra",
        ] {
            assert!(!is_cancellation(path));
        }
    }
    #[tokio::test]
    async fn cancellation_finishes_while_an_external_operation_holds_the_gate() {
        let gate = Arc::new(RwLock::new(()));
        let guard = gate.clone().write_owned().await;
        assert_eq!(
            detached(async { StatusCode::OK.into_response() })
                .await
                .status(),
            StatusCode::OK
        );
        assert!(gate.try_read().is_err());
        drop(guard);
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, finish) = tokio::sync::oneshot::channel();
        let (completed, done) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(detached(async move {
            started.send(()).unwrap();
            finish.await.unwrap();
            completed.send(()).unwrap();
            StatusCode::OK.into_response()
        }));
        ready.await.unwrap();
        request.abort();
        release.send(()).unwrap();
        done.await.unwrap();
    }
    #[tokio::test]
    async fn disconnection_keeps_the_reservation_until_the_operation_finishes() {
        let gate = Arc::new(RwLock::new(()));
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, finish) = tokio::sync::oneshot::channel();
        let (completed, done) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(run(gate.clone(), true, async move {
            started.send(()).unwrap();
            finish.await.unwrap();
            completed.send(()).unwrap();
            StatusCode::OK.into_response()
        }));
        ready.await.unwrap();
        request.abort();
        let refusal = run(gate.clone(), false, async {
            panic!("A second mutation must not start")
        })
        .await;
        assert_eq!(refusal.status(), StatusCode::CONFLICT);
        release.send(()).unwrap();
        done.await.unwrap();
        let guard = gate.write().await;
        drop(guard);
        assert_eq!(
            run(gate, false, async { StatusCode::OK.into_response() })
                .await
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn ordinary_edits_still_share_the_database_worker() {
        let gate = Arc::new(RwLock::new(()));
        let ordinary = gate.clone().read_owned().await;
        assert_eq!(
            run(gate.clone(), false, async {
                StatusCode::OK.into_response()
            })
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            run(gate, true, async {
                panic!("Git must wait for accepted database edits")
            })
            .await
            .status(),
            StatusCode::CONFLICT
        );
        drop(ordinary);
    }
}
