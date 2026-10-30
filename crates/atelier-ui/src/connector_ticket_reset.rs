use crate::{i18n::I18n, live::BoardState, write};
use atelier_core::application::extensions::{Connection, Method, TicketResetPreview};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ConnectorTicketReset(
    connection: Signal<Option<Connection>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let preview = RwSignal::new(None::<TicketResetPreview>);
    let approved = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let done = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let key = Memo::new(move |_| {
        connection
            .get()
            .map(|value| (value.installed.manifest.id, value.installed.revision))
    });
    Effect::new(move |_| {
        let _ = key.get();
        preview.set(None);
        approved.set(false);
        done.set(false);
    });
    Effect::new(move |_| dirty.set(preview.get().is_some()));
    let supported = move || {
        connection.get().is_some_and(|value| {
            [Method::TicketsReset, Method::TicketsSearch]
                .iter()
                .all(|method| value.installed.manifest.methods.contains(method))
        })
    };
    let execute = StoredValue::new_local(move |apply: bool| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(connection) = connection.get_untracked() else {
            return;
        };
        let profile = connection.installed;
        let expected = key.get_untracked();
        let body = if apply {
            if !approved.get_untracked() {
                return;
            }
            let Some(value) = preview.get_untracked() else {
                return;
            };
            serde_json::json!({"previewId":value.id,"expectedRevision":profile.revision,"confirmed":true})
        } else {
            serde_json::json!({"expectedRevision":profile.revision,"confirmed":true})
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/extensions/{}/tickets/reset/{}",
            profile.manifest.id,
            if apply { "apply" } else { "preview" }
        );
        let alive = alive.clone();
        pending.set(true);
        error.set(String::new());
        done.set(false);
        spawn_local(async move {
            let result = async {
                let value = write(&credential, Http::POST, &path, &body)
                    .await?
                    .json::<TicketResetPreview>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                if apply {
                    board.refresh(&credential).await?;
                }
                Ok::<_, String>(value)
            }
            .await;
            if alive.get() {
                pending.set(false);
                if key.get_untracked() == expected {
                    match result {
                        Ok(value) => {
                            approved.set(false);
                            if apply {
                                preview.set(None);
                                done.set(true);
                            } else {
                                preview.set(Some(value));
                            }
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    view! {
        <fieldset
            id="connector-reset"
            hidden={move || !supported()}
            disabled={move || pending.get() || blocked.get()}
        >
            <legend>{move || i18n.text("message.resynchronize_ticket_scope")}</legend>
            <p class="field-help">
                {move || {
                    i18n.text("message.read_every_page_before_confirmation_missing_tickets_will_be_mark")
                }}
            </p>
            <button
                id="preview-connector-ticket-reset"
                type="button"
                on:click={move |_| execute.with_value(|execute| execute(false))}
            >
                {move || i18n.text("message.read_the_complete_scope")}
            </button>
            {move || {
                preview
                    .get()
                    .map(|value| {
                        view! {
                            <p>
                                {format!(
                                    "{} · {} pages · {} tickets",
                                    value.observed_at,
                                    value.pages,
                                    value.tickets,
                                )}
                            </p>
                            <p hidden={!value
                                .rows
                                .is_empty()}>{i18n.text("message.no_linked_task_in_this_scope")}</p>
                            <ul>
                                {value
                                    .rows
                                    .into_iter()
                                    .map(|row| {
                                        view! {
                                            <li>
                                                {format!(
                                                    "{} · {} · {}",
                                                    row.key,
                                                    row.title,
                                                    if row.present {
                                                        i18n.text("message.in_scope")
                                                    } else {
                                                        i18n.text("message.out_of_scope")
                                                    },
                                                )}
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                    })
            }}
            <label hidden={move || preview.get().is_none()}>
                <input
                    id="approve-connector-ticket-reset"
                    type="checkbox"
                    prop:checked={move || approved.get()}
                    on:change={move |event| approved.set(event_target_checked(&event))}
                />
                {move || i18n.text("message.confirm_these_scope_flags_only_preserving_every_task")}
            </label>
            <button
                id="apply-connector-ticket-reset"
                type="button"
                disabled={move || preview.get().is_none() || !approved.get()}
                on:click={move |_| execute.with_value(|execute| execute(true))}
            >
                {move || i18n.text("message.apply_the_complete_scope")}
            </button>
            <button
                type="button"
                disabled={move || preview.get().is_none()}
                on:click={move |_| {
                    preview.set(None);
                    approved.set(false);
                }}
            >
                {move || i18n.text("message.cancel_preview")}
            </button>
            <p role="status" hidden={move || !pending.get()}>
                {move || i18n.text("message.complete_read_or_application_in_progress")}
            </p>
            <p role="status" hidden={move || !done.get()}>
                {move || i18n.text("message.scope_updated_tasks_and_their_text_were_preserved")}
            </p>
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
        </fieldset>
    }
}
