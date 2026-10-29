use super::{Shared, failure, object};
use crate::{Result, infrastructure::queue};
use axum::{
    extract::{Request, State},
    response::Response,
};
pub(super) async fn get(State(state): State<Shared>) -> Response {
    super::responses::response(state.database.call(queue::read).await)
}
pub(super) async fn change(State(state): State<Shared>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = state
        .database
        .call(move |store| queue::change(store, &body))
        .await;
    let _ = state.changed.send(());
    super::responses::response(result)
}
pub(super) async fn pause(State(state): State<Shared>, request: Request) -> Response {
    if let Err((status, message)) = object(request.into_body()).await {
        return failure(status, message);
    }
    let result = state
        .database
        .call(|store| queue::pause(store, "message.queue_paused_the_active_task_may_finish"))
        .await;
    let _ = state.changed.send(());
    super::responses::response(result)
}
pub(super) async fn run(state: Shared) -> Result<()> {
    let mut stopped = state.shutdown.subscribe();
    while !*stopped.borrow() {
        tokio::select! {_=stopped.changed()=>break,_=tokio::time::sleep(std::time::Duration::from_millis(500))=>{}}
        if !state.runs.active_cards().is_empty() {
            continue;
        }
        // This read-only hint can delay new work by one tick; all claims are revalidated under the gate.
        if matches!(state.database.call(queue::needs_tick).await, Ok(false)) {
            continue;
        }
        let Ok(_gate) = state.mutations.clone().try_write_owned() else {
            continue;
        };
        if *stopped.borrow() {
            break;
        }
        let operation = async {
            if state.database.call(queue::settle).await? {
                let _ = state.changed.send(());
            }
            if super::automation::tick(&state).await? { return Ok(()); }
            // The worker bypasses HTTP middleware but must honor its update reservation.
            if super::updates::guard(state.installation_root.as_deref()).is_some() {
                let changed = state.database.call(|store| {
                    if queue::read(store)?.paused { return Ok(false); }
                    queue::pause(store, "message.queue_paused_during_the_update_check_its_result_before_resuming")?;
                    Ok(true)
                }).await?;
                if changed { let _ = state.changed.send(()); }
                return Ok(());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let run_id = id.clone();
            let (claimed, changed) = state
                .database
                .call(move |store| {
                    let revision=queue::read(store)?.revision;
                    let claimed=queue::claim(store,id)?;
                    Ok((claimed,queue::read(store)?.revision!=revision))
                })
                .await?;
            if changed {
                let _ = state.changed.send(());
            }
            if let Some((card, body)) = claimed {
                if body.get::<bool>("workflow")?!=Some(true) {super::runs::launch(&state, card.id, body, run_id, true).await?;}
                let _ = state.changed.send(());
            }
            Ok::<(), crate::Error>(())
        }
        .await;
        if operation.is_err() {
            state
                .database
                .call(|store| {
                    queue::pause(
                        store,
                        "message.run_stopped_uncertain_or_failed_check_the_task_before_resuming_t",
                    )
                })
                .await?;
            let _ = state.changed.send(());
        }
    }
    if state
        .database
        .call(|store| Ok(queue::read(store)?.ids.is_empty()))
        .await?
    {
        return Ok(());
    }
    state
        .database
        .call(|store| {
            queue::pause(
                store,
                "message.queue_paused_after_restart_check_tasks_before_resuming",
            )
        })
        .await?;
    Ok(())
}
