use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::application::extensions::{
    self as protocol, Connection, Method, RemoteOperation, Report,
};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ConnectorTime(
    connection: Signal<Option<Connection>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let selected = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Report>);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let entry = Signal::derive(move || {
        board.times.with(|entries| {
            entries
                .iter()
                .find(|entry| field(entry, "id") == selected.get())
                .cloned()
        })
    });
    let card = Signal::derive(move || {
        entry.get().and_then(|entry| {
            board.cards.with(|cards| {
                cards
                    .iter()
                    .find(|card| card.id == field(&entry, "cardId"))
                    .cloned()
            })
        })
    });
    let eligible = Signal::derive(move || {
        board
            .times
            .get()
            .into_iter()
            .filter(|entry| {
                !entry.has_text("runningSince").unwrap_or(true)
                    && entry.string_equals("exportState", "draft").unwrap_or(false)
                    && atelier_core::domain::time::time_fields(entry).is_ok()
                    && board.cards.with(|cards| {
                        cards.iter().any(|card| {
                            card.id == field(entry, "cardId")
                                && !card.fields.flag("archived").unwrap_or(true)
                        })
                    })
            })
            .collect::<Vec<_>>()
    });
    let ready = Signal::derive(move || {
        eligible.with(|entries| {
            entries
                .iter()
                .any(|entry| field(entry, "id") == selected.get())
        })
    });
    let supported = move || {
        connection.get().is_some_and(|connection| {
            [Method::TimePreview, Method::TimeSend, Method::TimeReconcile]
                .iter()
                .all(|method| connection.installed.manifest.methods.contains(method))
        })
    };
    let key = Memo::new(move |_| {
        serde_json::json!({"module":connection.get().map(|connection|(connection.installed.manifest.id,connection.installed.revision)),"entry":entry.get(),"card":card.get()}).to_string()
    });
    Effect::new(move |_| {
        let _ = key.get();
        report.set(None);
        error.set(String::new());
    });
    Effect::new(move |_| dirty.set(!selected.get().is_empty() || report.get().is_some()));
    let execute = StoredValue::new_local(move |prepare: bool| {
        if pending.get_untracked() || blocked.get_untracked() || !ready.get_untracked() {
            return;
        }
        let (Some(connection), Some(entry), Some(card)) = (
            connection.get_untracked(),
            entry.get_untracked(),
            card.get_untracked(),
        ) else {
            return;
        };
        if entry.has_text("runningSince").unwrap_or(true)
            || !entry.string_equals("exportState", "draft").unwrap_or(false)
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let profile = connection.installed;
        let report_id = if prepare {
            let Some(report) = report.get_untracked() else {
                return;
            };
            Some(report.id)
        } else {
            None
        };
        let mut input = serde_json::json!({"expected":entry,"expectedCard":atelier_core::application::jira::card_snapshot(&card)});
        if let Some(id) = report_id {
            input["reportId"] = serde_json::json!(id);
        }
        let (path, body) = if prepare {
            (
                format!("/api/extensions/{}/operations", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":"timeSend","input":input}),
            )
        } else {
            (
                format!("/api/extensions/{}/invoke", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":"timePreview","action":null,"input":input,"confirmed":true}),
            )
        };
        let expected = key.get_untracked();
        let alive = alive.clone();
        pending.set(true);
        error.set(String::new());
        notice.set(false);
        spawn_local(async move {
            let result = async {
                let response = write(&credential, Http::POST, &path, &body).await?;
                if prepare {
                    response
                        .json::<RemoteOperation>()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())?;
                    Ok(None)
                } else {
                    response
                        .json::<Report>()
                        .await
                        .map(Some)
                        .map_err(|_| "message.invalid_server_response".to_owned())
                }
            }
            .await;
            if alive.get() {
                pending.set(false);
                if key.get_untracked() == expected {
                    match result {
                        Ok(Some(value)) => report.set(Some(value)),
                        Ok(None) => {
                            selected.set(String::new());
                            report.set(None);
                            notice.set(true);
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    view! {
        <fieldset hidden={move || !supported()} disabled={move || pending.get() || blocked.get()}>
            <legend>{move || i18n.text("message.export_a_time_entry_through_this_module")}</legend>
            <p class="field-help">
                {move || {
                    i18n.text("message.choose_a_stopped_human_time_entry_the_module_reads_its_destinati")
                }}
            </p>
            <p hidden={move || {
                !eligible.with(Vec::is_empty)
            }}>{move || i18n.text("message.no_stopped_unsent_human_time_entry_is_available")}</p>
            <label for="connector-time-entry">{move || i18n.text("message.time_entry")}</label>
            <select
                id="connector-time-entry"
                prop:value={move || selected.get()}
                on:change={move |event| selected.set(event_target_value(&event))}
            >
                <option value="">{move || i18n.text("message.choose_an_entry")}</option>
                {move || {
                    eligible
                        .get()
                        .into_iter()
                        .map(|entry| {
                            let id = field(&entry, "id");
                            let title = board
                                .cards
                                .with(|cards| {
                                    cards
                                        .iter()
                                        .find(|card| card.id == field(&entry, "cardId"))
                                        .map(|card| card.title.clone())
                                        .unwrap_or_default()
                                });
                            view! {
                                <option value={id}>
                                    {format!(
                                        "{} {} · {} s · {}",
                                        field(&entry, "date"),
                                        field(&entry, "startTime"),
                                        atelier_core::domain::time::time_fields(&entry)
                                            .map(|fields| fields.seconds)
                                            .unwrap_or(0),
                                        title,
                                    )}
                                </option>
                            }
                        })
                        .collect_view()
                }}
            </select>
            <button
                id="preview-connector-time"
                type="button"
                disabled={move || !ready.get()}
                on:click={move |_| execute.with_value(|execute| execute(false))}
            >
                {move || i18n.text("message.read_the_export_destination")}
            </button>
            {move || {
                report
                    .get()
                    .and_then(|report| protocol::time_proposal(&report.output).ok())
                    .map(|proposal| {
                        view! {
                            <dl>
                                <dt>{i18n.text("message.destination")}</dt>
                                <dd>{proposal.instance}</dd>
                                <dt>{i18n.text("message.account")}</dt>
                                <dd>{proposal.account}</dd>
                                <dt>{i18n.text("message.remote_reference")}</dt>
                                <dd>{proposal.reference}</dd>
                                {proposal
                                    .source
                                    .ticket
                                    .map(|ticket| {
                                        view! {
                                            <dt>{i18n.text("message.source_ticket")}</dt>
                                            <dd>{format!("{} · {}", ticket.key, ticket.instance)}</dd>
                                        }
                                    })}
                                <dt>{i18n.text("message.duration")}</dt>
                                <dd>{format!("{} s", proposal.source.seconds)}</dd>
                                <dt>{i18n.text("message.description")}</dt>
                                <dd class="pre-wrap">{proposal.source.description}</dd>
                            </dl>
                        }
                    })
            }}
            <button
                id="prepare-connector-time"
                type="button"
                disabled={move || report.get().is_none() || !ready.get()}
                on:click={move |_| execute.with_value(|execute| execute(true))}
            >
                {move || i18n.text("message.prepare_this_export_in_the_journal")}
            </button>
            <button
                type="button"
                disabled={move || selected.get().is_empty()}
                on:click={move |_| {
                    selected.set(String::new());
                    report.set(None);
                }}
            >
                {move || i18n.text("message.cancel_selection")}
            </button>
            <p role="status" hidden={move || !notice.get()}>
                {move || {
                    i18n.text("message.preview_added_to_the_journal_check_its_scope_then_confirm_sendin")
                }}
            </p>
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
        </fieldset>
    }
}
