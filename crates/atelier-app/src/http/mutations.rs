use super::{Shared, failure, runs, services, updates};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::Response,
};
use std::{future::Future, sync::Arc};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Policy {
    Ordinary,
    External,
    ProjectExternal,
    Cancellation,
}

impl Policy {
    pub(super) fn exclusive(self) -> bool {
        matches!(self, Self::External | Self::ProjectExternal)
    }

    pub(super) fn project_guard(self) -> bool {
        self == Self::ProjectExternal
    }
}

pub(super) trait ApiRoutes {
    fn api_route(
        self,
        path: &str,
        policy: Policy,
        handler: axum::routing::MethodRouter<super::Shared>,
    ) -> Self;
}

impl ApiRoutes for axum::Router<super::Shared> {
    fn api_route(
        self,
        path: &str,
        policy: Policy,
        handler: axum::routing::MethodRouter<super::Shared>,
    ) -> Self {
        self.route(
            path,
            handler.layer(axum::middleware::from_fn_with_state(policy, mutate)),
        )
    }
}

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
            "message.a_change_is_in_progress_try_again_shortly",
        );
    };
    detached(async move {
        let _guard = guard;
        operation.await
    })
    .await
}

// Accepted writes and cancellations must finish even when the browser disconnects.
pub(super) async fn detached(
    operation: impl Future<Output = Response> + Send + 'static,
) -> Response {
    match tokio::spawn(operation).await {
        Ok(response) => response,
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.the_change_was_interrupted_refresh_the_current_state_before_tryi",
        ),
    }
}

pub(super) async fn mutate(
    State(policy): State<Policy>,
    axum::Extension(state): axum::Extension<Shared>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::GET {
        let cancellation = request.method() == Method::POST && policy == Policy::Cancellation;
        let gate = state.mutations.clone();
        let changed = state.changed.clone();
        let services = state.services.clone();
        let runs = state.runs.clone();
        let operation = async move {
            if !cancellation {
                if let Some(response) = updates::guard(state.installation_root.as_deref()) {
                    return response;
                }
                let guard_path = if policy.project_guard() {
                    "/api/projects"
                } else {
                    request.uri().path()
                };
                if let Some(response) = services::guard(&services, guard_path) {
                    return response;
                }
                if let Some(response) = runs::guard(&runs, guard_path) {
                    return response;
                }
                let guarded_path = guard_path.to_owned();
                let operation_path = request.uri().path().to_owned();
                match state
                    .database
                    .call(move |store| {
                        let tempo_path =
                            if crate::infrastructure::connector_effects::reconciles_time(
                                store,
                                &operation_path,
                            )? {
                                &operation_path
                            } else {
                                &guarded_path
                            };
                        Ok(crate::infrastructure::connector_effects::blocks(
                            store,
                            &operation_path,
                        )? || atelier_core::application::agent_cli::busy(
                            &store.settings::<atelier_core::document::Document>()?,
                        )? || crate::infrastructure::jira_transition::blocks(
                            store,
                            &guarded_path,
                        )? || crate::infrastructure::tempo_export::blocks(store, tempo_path)?
                            || crate::infrastructure::forge_draft::blocks(store, &guarded_path)?
                            || crate::infrastructure::automation::blocks(store, &guarded_path)?)
                    })
                    .await
                {
                    Ok(true) => {
                        return failure(
                            StatusCode::CONFLICT,
                            "message.finish_or_inspect_the_current_operation_before_changing_this_tas",
                        );
                    }
                    Err(_) => {
                        return failure(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "message.unable_to_read_jira_operations",
                        );
                    }
                    Ok(false) => {}
                }
            }
            let response = next.run(request).await;
            if response.status().is_success() {
                let _ = changed.send(());
            }
            response
        };
        if cancellation {
            detached(operation).await
        } else {
            run(gate, policy.exclusive(), operation).await
        }
    } else {
        next.run(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn every_api_registration_declares_its_concurrency_policy() {
        let routes = include_str!("routes.rs");
        assert!(!routes.contains(".route("));
        assert!(Policy::External.exclusive());
        assert!(Policy::ProjectExternal.exclusive());
        assert!(!Policy::Ordinary.exclusive());
        assert!(!Policy::Cancellation.exclusive());
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
