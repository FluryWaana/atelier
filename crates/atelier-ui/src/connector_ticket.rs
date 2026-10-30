use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    application::extensions::{
        self as protocol, Method as ConnectorMethod, Registry, Report, Ticket,
    },
    contracts::Card,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use serde_json::json;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[path = "connector_ticket_transitions.rs"]
mod connector_ticket_transitions;

#[derive(Clone)]
struct Preview {
    expected: Card,
    revision: u64,
    report: Report,
    ticket: Ticket,
}

#[component]
pub fn ConnectorTicket(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
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
    let module = Signal::derive(move || {
        card.get()
            .and_then(|card| {
                card.fields
                    .get::<atelier_core::document::Document>("ticket")
                    .ok()
                    .flatten()
            })
            .and_then(|ticket| {
                field(&ticket, "connector")
                    .strip_prefix("extension:")
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    });
    let preview = RwSignal::new(None::<Preview>);
    let replace_title = RwSignal::new(false);
    let replace_description = RwSignal::new(false);
    let approved = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let transition_dirty = RwSignal::new(false);
    Effect::new(move |_| dirty.set(preview.get().is_some() || transition_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let stale = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            card.get().is_none_or(|card| {
                serde_json::to_value(atelier_core::application::jira::card_snapshot(&card)).ok()
                    != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                        &preview.expected,
                    ))
                    .ok()
            })
        })
    });
    let load = move |_| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let module = module.get_untracked();
        let Ok(credential) = token.get_value() else {
            return;
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(false);
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                let registry = read(&credential,"/api/extensions").await?.json::<Registry>().await.map_err(|_| "message.invalid_server_response".to_owned())?;
                let profile = registry.modules.iter().find(|connection| connection.installed.manifest.id == module).ok_or("message.reinstall_this_ticket_s_connector_before_synchronizing_it")?;
                if !registry.native_available || !profile.installed.manifest.methods.contains(&ConnectorMethod::TicketsRead) { return Err("message.this_connector_cannot_read_the_ticket_on_this_platform".into()); }
                let link = expected.fields.get::<atelier_core::document::Document>("ticket").map_err(|_|"message.invalid_connector_ticket_eb5f5c57")?.ok_or("message.no_connector_ticket_is_linked")?;
                let revision = profile.installed.revision;
                let response = write(&credential,Method::POST,&format!("/api/extensions/{module}/invoke"),&json!({"expectedRevision":revision,"method":"ticketsRead","action":null,"input":{"instance":field(&link,"instance"),"id":field(&link,"id")},"confirmed":true})).await?;
                let report = response.json::<Report>().await.map_err(|_|"message.invalid_server_response".to_owned())?;
                let page = protocol::ticket_page(report.method,&report.output).map_err(str::to_owned)?;
                if report.method != ConnectorMethod::TicketsRead || page.items.len() != 1 || page.items[0].instance != field(&link,"instance") || page.items[0].id != field(&link,"id") { return Err("message.the_report_does_not_match_the_ticket_linked_to_this_card".into()); }
                Ok::<_,String>(Preview { expected, revision, report, ticket:page.items[0].clone() })
            }.await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(value) => {
                    preview.set(Some(value));
                    replace_title.set(false);
                    replace_description.set(false);
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    let apply = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || stale.get_untracked()
            || !approved.get_untracked()
        {
            return;
        }
        let Some(snapshot) = preview.get_untracked() else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let body = json!({"expectedRevision":snapshot.revision,"reportId":snapshot.report.id,"expected":snapshot.expected,"replaceTitle":replace_title.get_untracked(),"replaceDescription":replace_description.get_untracked(),"confirmed":true});
        pending.set(true);
        approved.set(false);
        error.set(String::new());
        let alive = mounted.get_value();
        spawn_local(async move {
            let result = async {
                let response = write(
                    &credential,
                    Method::POST,
                    &format!("/api/cards/{}/connector-refresh", id.get_value()),
                    &body,
                )
                .await?;
                response
                    .json::<Card>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            let _ = board.refresh(&credential).await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(card) => {
                    preview.set(None);
                    on_applied.run(card);
                    notice.set(
                        "message.ticket_synchronized_other_local_fields_were_preserved".into(),
                    );
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    view! { <section class="task-files" hidden=move||module.get().is_empty() aria-labelledby="connector-ticket-heading">
        <h3 id="connector-ticket-heading">{move||i18n.text("message.connector_ticket_synchronization")}</h3>
        <p role="status" hidden=move||!card.get().is_some_and(|card|card.fields.flag("ticketOutOfScope").unwrap_or(false))>{move||i18n.text("message.this_ticket_is_outside_the_module_s_last_complete_scope_its_task")}</p>
        <button id="read-connector-ticket" type="button" disabled=move||pending.get()||blocked.get()||transition_dirty.get() on:click=load>{move||i18n.text("message.read_remote_ticket")}</button>
        <Show when=move||preview.get().is_some()>
            <p>{move||preview.get().map(|preview|format!("{} · {}",preview.ticket.key,preview.report.observed_at)).unwrap_or_default()}</p>
            <p class="field-help">{move||i18n.text("message.choose_which_text_to_replace_the_ticket_status_will_be_refreshed")}</p>
            <label><input id="replace-connector-ticket-title" type="checkbox" disabled=move||pending.get()||blocked.get() prop:checked=move||replace_title.get() on:change=move|event|{replace_title.set(event_target_checked(&event));approved.set(false);}/>{move||i18n.text("message.replace_local_title")}</label>
            <p class="task-description">{move||preview.get().map(|preview|preview.ticket.title).unwrap_or_default()}</p>
            <label><input id="replace-connector-ticket-description" type="checkbox" disabled=move||pending.get()||blocked.get() prop:checked=move||replace_description.get() on:change=move|event|{replace_description.set(event_target_checked(&event));approved.set(false);}/>{move||i18n.text("message.replace_local_description")}</label>
            <p class="task-description">{move||preview.get().map(|preview|preview.ticket.description).unwrap_or_default()}</p>
            <p>{move||preview.get().map(|preview|preview.ticket.status).unwrap_or_default()}</p>
            <p class="error" role="alert" hidden=move||!stale.get()>{move||i18n.text("message.the_task_changed_read_the_ticket_again_before_confirming")}</p>
            <label><input id="approve-connector-ticket-refresh" type="checkbox" disabled=move||pending.get()||blocked.get()||stale.get() prop:checked=move||approved.get()&&!stale.get() on:change=move|event|approved.set(event_target_checked(&event))/>{move||i18n.text("message.apply_this_preview_with_the_selected_fields")}</label>
            <button id="apply-connector-ticket-refresh" type="button" disabled=move||pending.get()||blocked.get()||stale.get()||!approved.get() on:click=apply>{move||i18n.text("message.synchronize_ticket")}</button>
            <button id="cancel-connector-ticket-refresh" type="button" disabled=move||pending.get() on:click=move|_|{preview.set(None);approved.set(false);error.set(String::new());}>{move||i18n.text("message.cancel_preview")}</button>
        </Show>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p><p role="status">{move||i18n.text(&notice.get())}</p>
        <connector_ticket_transitions::ConnectorTicketTransitions card_id=id.get_value() card module board token pending dirty=transition_dirty blocked=Memo::new(move |_|blocked.get()||preview.get().is_some())/>
    </section> }
}
