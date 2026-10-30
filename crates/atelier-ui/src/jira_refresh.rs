use crate::{field, i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, JiraConfiguration, JiraRefreshApply, JiraRefreshPreview, JiraRefreshRequest,
    JiraRefreshState as Phase,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn JiraRefresh(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    on_applied: Callback<Card>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<JiraRefreshPreview>("jiraRefresh")
                .ok()
                .flatten()
        })
    });
    let linked = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref().is_some_and(|card| {
                card.fields
                    .get::<atelier_core::document::Document>("ticket")
                    .ok()
                    .flatten()
                    .is_some_and(|ticket| field(&ticket, "connector") == "jira")
            })
        })
    });
    let busy = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref().is_some_and(|card| {
                atelier_core::application::jira::unresolved(&card.fields).unwrap_or(true)
            })
        })
    });
    let revision = move || {
        board.settings.with(|settings| {
            settings
                .get::<JiraConfiguration>("jiraConnection")
                .ok()
                .flatten()
                .map(|config| config.revision)
                .unwrap_or(0)
        })
    };
    let confirmed = RwSignal::new(String::new());
    let apply_status = RwSignal::new(false);
    let preview_context = Memo::new(move |_| {
        (
            preview.get().map(|preview| preview.id),
            serde_json::to_string(&board.settings.get()).unwrap_or_default(),
            serde_json::to_string(&board.projects.get()).unwrap_or_default(),
        )
    });
    Effect::new(move |_| {
        let _ = preview_context.get();
        confirmed.set(String::new());
        apply_status.set(false);
    });
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let loading_alive = alive.clone();
    let load = move |_| {
        if pending.get_untracked() || blocked.get_untracked() || busy.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(JiraRefreshRequest {
            revision: revision(),
            expected: atelier_core::application::jira::card_snapshot(&expected),
        })
        .expect("Jira refresh serialization");
        pending.set(true);
        confirmed.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = loading_alive.clone();
        let path = format!("/api/cards/{}/jira/refresh-preview", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => replace_card(board.cards, card),
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    let applying_alive = alive.clone();
    let apply = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || busy.get_untracked()
            || confirmed.get_untracked().is_empty()
        {
            return;
        }
        let (Some(expected), Some(preview)) = (card.get_untracked(), preview.get_untracked())
        else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if confirmed.get_untracked() != preview.id {
            return;
        }
        let body = serde_json::to_value(JiraRefreshApply {
            revision: revision(),
            expected: atelier_core::application::jira::card_snapshot(&expected),
            preview_id: preview.id,
            apply_status: apply_status.get_untracked(),
        })
        .expect("Jira refresh apply serialization");
        pending.set(true);
        confirmed.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = applying_alive.clone();
        let path = format!("/api/cards/{}/jira/refresh-apply", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => {
                        on_applied.run(card.clone());
                        replace_card(board.cards, card);
                        notice.set("message.card_synchronized_from_the_jira_preview_no_remote_issue_was_chan".into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    view! {<section class="task-files" hidden=move ||!linked.get() aria-labelledby="jira-refresh-heading" aria-busy=move ||pending.get().to_string()>
        <h3 id="jira-refresh-heading">{move ||i18n.text("message.synchronize_jira_content")}</h3><p class="field-help">{move ||i18n.text("message.read_the_issue_again_compare_the_preview_then_explicitly_apply_i")}</p>
        <button id="load-jira-refresh" type="button" disabled=move ||pending.get()||blocked.get()||busy.get() on:click=load>{move ||i18n.text("message.read_the_issue_again_to_compare")}</button>
        <p role="status" hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Reading))>{move ||i18n.text("message.reading_the_issue_to_prepare_the_preview")}</p>
        <p role="status" hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Interrupted))>{move ||i18n.text("message.the_previous_read_was_interrupted_you_can_read_the_issue_again")}</p>
        {move ||preview.get().filter(|preview|preview.state==Phase::Ready).and_then(|preview|preview.detail.map(|detail|view!{<div><h4>{move ||i18n.text("message.current_card_content")}</h4><p>{preview.expected.title.clone()}</p><pre>{field(&preview.expected.fields,"description")}</pre><h4>{move ||i18n.text("message.content_read_from_jira")}</h4><p>{detail.issue.title}</p><pre>{detail.description}</pre></div>}))}
        <div hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Ready))><p class="field-help">{move ||i18n.text("message.the_preview_expires_after_ten_minutes_a_concurrent_change_requir")}</p><label><input id="confirm-jira-refresh" type="checkbox" disabled=move ||pending.get()||blocked.get()||busy.get() prop:checked=move ||preview.with(|preview|preview.as_ref().is_some_and(|preview|preview.id==confirmed.get())) on:change=move |event|confirmed.set(if event_target_checked(&event){preview.with(|preview|preview.as_ref().map(|preview|preview.id.clone()).unwrap_or_default())}else{String::new()}) />{move ||i18n.text("message.replace_the_local_title_and_description_with_this_jira_preview")}</label><button id="apply-jira-refresh" type="button" disabled=move ||pending.get()||blocked.get()||busy.get()||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.id!=confirmed.get())) on:click=apply>{move ||i18n.text("message.apply_this_preview_to_the_card")}</button></div>
        {move||preview.get().filter(|preview|preview.state==Phase::Ready).and_then(|preview|preview.mapped_status).map(|status| {
            let label=move||{
                let repo=card.get().map(|card|field(&card.fields,"repo")).unwrap_or_default();
                board.projects.with(|projects|crate::kanban_settings::project_column_name(i18n,projects,&repo,&status))
            };
            view!{<label><input id="jira-refresh-status" type="checkbox" disabled=move||pending.get()||blocked.get()||busy.get() prop:checked=move||apply_status.get() on:change=move|event|{apply_status.set(event_target_checked(&event));confirmed.set(String::new());}/>{move||i18n.text("message.also_apply_the_proposed_column")}{label}</label>}
        })}
        <p class="error" role="alert">{move ||i18n.text(&error.get())}</p><p role="status">{move ||i18n.text(&notice.get())}</p>
    </section>}
}
