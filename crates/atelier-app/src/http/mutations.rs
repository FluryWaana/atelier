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
    // A disconnected browser must not release a reservation while Git/SQLite still changes state.
    match tokio::spawn(async move {
        let _guard = guard;
        operation.await
    })
    .await
    {
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
