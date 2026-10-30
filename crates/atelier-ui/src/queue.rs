use crate::{i18n::I18n, live::BoardState, write};
use atelier_core::contracts::RunQueue;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn Queue(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_open: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let approved = RwSignal::new(None::<u64>);
    let state = Signal::derive(move || {
        board.settings.with(|settings| {
            settings
                .get::<RunQueue>("runQueue")
                .ok()
                .flatten()
                .unwrap_or_default()
        })
    });
    let act = StoredValue::new_local(move |action: &str, id: Option<String>| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let queue = state.get_untracked();
        if action == "resume" && approved.get_untracked() != Some(queue.revision) {
            return;
        }
        let path = if action == "pause" {
            "/api/queue/pause"
        } else {
            "/api/queue"
        };
        let body = serde_json::json!({"action":action,"cardId":id,"revision":queue.revision,"confirmed":true});
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(&credential, Method::POST, path, &body).await;
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                approved.set(None);
                busy.set(false);
            }
        });
    });
    view! {<details class="task-files" id="run-queue"><summary>{move||format!("{} ({})",i18n.text("message.run_queue"),state.get().ids.len())}</summary><p role="status">{move||i18n.text(if state.get().paused{"message.queue_paused"}else{"message.queue_active"})}</p><p class="field-help">{move||i18n.text("message.tasks_and_workflows_follow_the_displayed_order_using_their_setti")}</p><p role="status">{move||i18n.text(&state.get().message)}</p>
    <p hidden=move||!state.get().ids.is_empty()>{move||i18n.text("message.no_queued_tasks_add_a_task_from_its_runs_panel")}</p>
    <For each=move||state.get().ids key=|id|id.clone() children=move|id|{let selected=StoredValue::new(id);view!{<div class="editor-actions"><button type="button" on:click=move|_|on_open.run(selected.get_value())>{move||board.cards.with(|cards|cards.iter().find(|card|card.id==selected.get_value()).map(|card|card.title.clone()).unwrap_or_else(||i18n.text("message.task_not_found")))}</button><small hidden=move||!state.get().workflows.contains_key(&selected.get_value())>{move||i18n.text("message.project_workflow")}</small><button class="remove-queued-task" type="button" disabled=move||busy.get()||state.get().active.is_some_and(|active|active.card_id==selected.get_value()) on:click=move|_|act.with_value(|act|act("remove",Some(selected.get_value())))>{move||i18n.text("message.remove_from_queue")}</button></div>}} />
    <label class="check-filter" hidden=move||!state.get().paused||state.get().ids.is_empty()><input id="confirm-resume-queue" type="checkbox" prop:checked=move||approved.get()==Some(state.get().revision) on:change=move|event|approved.set(event_target_checked(&event).then(||state.get_untracked().revision)) />{move||i18n.text("message.authorize_local_launches_and_resumptions_with_the_saved_settings")}</label>
    <button id="resume-queue" type="button" disabled=move||busy.get()||!state.get().paused||state.get().ids.is_empty()||approved.get()!=Some(state.get().revision) on:click=move|_|act.with_value(|act|act("resume",None))>{move||i18n.text("message.resume_queue")}</button><button id="pause-queue" type="button" disabled=move||busy.get()||state.get().paused on:click=move|_|act.with_value(|act|act("pause",None))>{move||i18n.text("message.pause_queue")}</button><p class="error" role="alert">{move||i18n.text(&error.get())}</p></details>}
}
