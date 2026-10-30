use crate::{field, i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::{
    application::jira as domain,
    contracts::{
        Card, JiraConfiguration, JiraCreateRequest, JiraCreationIntent, JiraCreationMetadata,
        JiraCreationResolve, JiraIssueType, JiraWriteState as Phase,
    },
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, Copy)]
enum Action {
    Create,
    Link,
    Close,
}
#[component]
pub fn JiraCreation(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    archived: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<JiraCreationIntent>("jiraCreation")
                .ok()
                .flatten()
        })
    });
    let linked = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref()
                .is_some_and(|card| card.fields.has_object("ticket").unwrap_or(false))
        })
    });
    let uncertain = Memo::new(move |_| {
        intent.with(|intent| {
            intent
                .as_ref()
                .is_some_and(|intent| intent.state == Phase::Uncertain)
        })
    });
    let writing = Memo::new(move |_| {
        intent.with(|intent| {
            intent
                .as_ref()
                .is_some_and(|intent| matches!(intent.state, Phase::Preparing | Phase::Sending))
        })
    });
    let project = RwSignal::new(String::new());
    let types = RwSignal::new(Vec::<JiraIssueType>::new());
    let selected = RwSignal::new(String::new());
    let prepared = RwSignal::new(None::<(Card, JiraCreationMetadata, u64)>);
    let sequence = RwSignal::new(0u64);
    let approved = RwSignal::new(None::<u64>);
    let key = RwSignal::new(String::new());
    let close_ack = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    Effect::new(move |_| dirty.set(!project.get().is_empty() || !key.get().is_empty()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let configuration = move || {
        board.settings.with(|settings| {
            settings
                .get::<JiraConfiguration>("jiraConnection")
                .ok()
                .flatten()
                .unwrap_or_default()
        })
    };
    let validation = Signal::derive(move || {
        prepared.with(|prepared| {
            prepared
                .as_ref()
                .and_then(|(card, metadata, _)| domain::validate_creation(metadata, card).err())
                .map(|error| error.to_string())
                .unwrap_or_default()
        })
    });
    let reading_alive = alive.clone();
    let load_types = move |_| {
        if pending.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || uncertain.get_untracked()
            || !domain::project_key(&project.get_untracked())
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
        let revision = configuration().revision;
        let requested = project.get_untracked();
        loading.set(true);
        types.set(Vec::new());
        selected.set(String::new());
        prepared.set(None);
        approved.set(None);
        error.set(String::new());
        notice.set(String::new());
        let alive = reading_alive.clone();
        spawn_local(async move {
            let result = async {
                let response = read(
                    &credential,
                    &format!("/api/jira/creation-metadata?revision={revision}&project={requested}"),
                )
                .await?
                .json::<Document>()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())?;
                if response.get::<u64>("revision").ok().flatten() != Some(revision)
                    || field(&response, "projectKey") != requested
                {
                    return Err("message.jira_creation_metadata_is_stale".to_owned());
                }
                response
                    .get::<Vec<JiraIssueType>>("types")
                    .map_err(|_| "message.invalid_jira_issue_types".to_owned())?
                    .ok_or("message.missing_jira_issue_types".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(value) => types.set(value),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let preparing_alive = alive.clone();
    let prepare = move |_| {
        if pending.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || uncertain.get_untracked()
        {
            return;
        }
        let Some(source) = card.get_untracked() else {
            return;
        };
        let source = domain::card_snapshot(&source);
        let Some(kind) = types.with_untracked(|types| {
            types
                .iter()
                .find(|kind| kind.id == selected.get_untracked() && !kind.subtask)
                .cloned()
        }) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let revision = configuration().revision;
        let requested = project.get_untracked();
        loading.set(true);
        prepared.set(None);
        approved.set(None);
        error.set(String::new());
        let alive = preparing_alive.clone();
        spawn_local(async move {
            let result=async{let metadata=read(&credential,&format!("/api/jira/creation-metadata?revision={revision}&project={requested}&issueType={}",kind.id)).await?.json::<JiraCreationMetadata>().await.map_err(|_|"message.invalid_server_response".to_owned())?;
            if metadata.revision!=revision||metadata.project_key!=requested||metadata.issue_type!=kind{return Err("message.jira_creation_metadata_is_stale".to_owned());}Ok(metadata)}.await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(metadata) => {
                        sequence.update(|value| *value = value.wrapping_add(1));
                        prepared.set(Some((source, metadata, sequence.get_untracked())));
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let execute = StoredValue::new_local(move |action: Action, body: serde_json::Value| {
        if pending.get_untracked() || loading.get_untracked() || blocked.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/cards/{}/jira/{}",
            id.get_value(),
            if matches!(action, Action::Create) {
                "create"
            } else {
                "creation-reconcile"
            }
        );
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
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
                        let phase = card
                            .fields
                            .get::<JiraCreationIntent>("jiraCreation")
                            .ok()
                            .flatten()
                            .map(|intent| intent.state);
                        replace_card(board.cards, card);
                        project.set(String::new());
                        types.set(Vec::new());
                        selected.set(String::new());
                        prepared.set(None);
                        approved.set(None);
                        key.set(String::new());
                        close_ack.set(String::new());
                        notice.set(match(action,phase){(Action::Create,Some(Phase::Confirmed))=>"message.issue_created_and_linked_to_the_card_its_status_still_needs_to_b",(Action::Link,Some(Phase::Confirmed))=>"message.issue_found_and_linked_no_new_issue_created",(Action::Close,Some(Phase::Dismissed))=>"message.uncertainty_closed_after_verification_no_remote_issue_deleted",_=>"message.creation_or_linking_is_unconfirmed_check_jira_before_trying_agai"}.into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    });
    let create = move |_| {
        let Some((source, metadata, proposal)) = prepared.get_untracked() else {
            return;
        };
        if approved.get_untracked() != Some(proposal) || !validation.get_untracked().is_empty() {
            return;
        }
        let body = serde_json::to_value(JiraCreateRequest {
            revision: metadata.revision,
            expected: source,
            project_key: metadata.project_key,
            issue_type: metadata.issue_type,
            confirmed: true,
        })
        .expect("Jira creation serialization");
        execute.with_value(|execute| execute(Action::Create, body));
    };
    let reconcile = move |_| {
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let body = serde_json::to_value(JiraCreationResolve {
            revision: configuration().revision,
            expected: domain::card_snapshot(&expected),
            existing_key: (!key.get_untracked().trim().is_empty())
                .then(|| key.get_untracked().trim().to_ascii_uppercase()),
            close_uncertainty: false,
        })
        .expect("Jira creation reconciliation serialization");
        execute.with_value(|execute| execute(Action::Link, body));
    };
    let close_uncertainty = move |_| {
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(current) = intent.get_untracked() else {
            return;
        };
        if close_ack.get_untracked() != current.id {
            return;
        }
        let body = serde_json::to_value(JiraCreationResolve {
            revision: configuration().revision,
            expected: domain::card_snapshot(&expected),
            existing_key: None,
            close_uncertainty: true,
        })
        .expect("Jira creation close serialization");
        execute.with_value(|execute| execute(Action::Close, body));
    };
    view! {
        <section
            class="task-files"
            hidden={move || {
                (linked.get()
                    || !board.settings.get().string_equals("ticketProvider", "jira").unwrap_or(false))
                    && intent.with(|intent| intent.is_none())
            }}
            aria-labelledby="jira-create-heading"
            aria-busy={move || (pending.get() || loading.get()).to_string()}
        >
            <h3 id="jira-create-heading">{move || i18n.text("message.create_a_jira_issue")}</h3>
            <p class="field-help">
                {move || {
                    i18n.text("message.prepare_an_issue_from_this_cards_saved_title_description_criteri")
                }}
            </p>
            <p role="status">
                {move || {
                    intent
                        .get()
                        .map(|intent| {
                            i18n.text(
                                match intent.state {
                                    Phase::Preparing => "message.preparing_jira_creation",
                                    Phase::Sending => "message.jira_creation_request_in_progress",
                                    Phase::Uncertain => {
                                        "message.jira_creation_is_uncertain_no_automatic_resubmission"
                                    }
                                    Phase::Confirmed => "message.jira_issue_link_saved",
                                    Phase::Failed => "message.jira_preparation_interrupted_before_sending",
                                    Phase::Dismissed => "message.jira_uncertainty_closed_after_verification",
                                },
                            )
                        })
                        .unwrap_or_default()
                }}
            </p>
            {move || {
                intent
                    .get()
                    .and_then(|intent| intent.result)
                    .map(|issue| {
                        view! {
                            <p>
                                <a href={issue.url} target="_blank" rel="noopener noreferrer">
                                    {issue.key}
                                </a>
                            </p>
                        }
                    })
            }}
            <fieldset
                hidden={move || linked.get() || uncertain.get() || writing.get()}
                disabled={move || {
                    pending.get() || loading.get() || blocked.get() || archived
                        || !board.settings.get().string_equals("ticketProvider", "jira").unwrap_or(false)
                }}
            >
                <crate::form::Field
                    id="jira-create-project"
                    label={Signal::derive(move || i18n.text("message.jira_project_key"))}
                >
                    <crate::form::Input
                        {..}
                        id="jira-create-project"
                        maxlength="64"
                        placeholder="SYN"
                        prop:value={move || project.get()}
                        on:input={move |event| {
                            project.set(event_target_value(&event).trim().to_ascii_uppercase());
                            types.set(Vec::new());
                            selected.set(String::new());
                            prepared.set(None);
                            approved.set(None);
                        }}
                    />
                </crate::form::Field>
                <crate::button::Button
                    disabled={Signal::derive(move || !domain::project_key(&project.get()))}
                    {..}
                    id="load-jira-types"
                    type="button"
                    on:click={load_types}
                >
                    {move || i18n.text("message.load_issue_types")}
                </crate::button::Button>
                <crate::form::Field
                    id="jira-create-type"
                    label={Signal::derive(move || i18n.text("message.ticket_type"))}
                >
                    <crate::form::Select
                        {..}
                        id="jira-create-type"
                        prop:value={move || selected.get()}
                        on:change={move |event| {
                            selected.set(event_target_value(&event));
                            prepared.set(None);
                            approved.set(None);
                        }}
                    >
                        <option value="">{move || i18n.text("message.choose_an_issue_type")}</option>
                        <For
                            each={move || types.get()}
                            key={|kind| kind.id.clone()}
                            children={move |kind| {
                                view! {
                                    <option value={kind.id} disabled={kind.subtask}>
                                        {kind.name}
                                    </option>
                                }
                            }}
                        />
                    </crate::form::Select>
                </crate::form::Field>
                <p class="field-help">
                    {move || {
                        i18n.text("message.create_subtasks_and_issues_requiring_additional_fields_in_jira_t")
                    }}
                </p>
                <crate::button::Button
                    disabled={Signal::derive(move || selected.get().is_empty())}
                    {..}
                    id="prepare-jira-create"
                    type="button"
                    on:click={prepare}
                >
                    {move || i18n.text("message.prepare_issue_preview")}
                </crate::button::Button>
                {move || {
                    prepared
                        .get()
                        .map(|(source, _, _)| {
                            view! {
                                <div>
                                    <h4>{source.title.clone()}</h4>
                                    <pre class="service-log">
                                        {domain::creation_text(&source).unwrap_or_default()}
                                    </pre>
                                </div>
                            }
                        })
                }}
                <p class="error" role="alert">
                    {move || i18n.text(&validation.get())}
                </p>
                <label hidden={move || prepared.with(|prepared| prepared.is_none())}>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-jira-create"
                        prop:checked={move || {
                            prepared
                                .with(|prepared| {
                                    prepared
                                        .as_ref()
                                        .is_some_and(|(_, _, proposal)| approved.get() == Some(*proposal))
                                })
                        }}
                        on:change={move |event| {
                            approved
                                .set(
                                    if event_target_checked(&event) {
                                        prepared
                                            .with(|prepared| {
                                                prepared.as_ref().map(|(_, _, proposal)| *proposal)
                                            })
                                    } else {
                                        None
                                    },
                                )
                        }}
                    />
                    {move || {
                        i18n.text("message.create_this_issue_in_the_selected_jira_project_and_link_it_to_th")
                    }}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        prepared
                            .with(|prepared| {
                                prepared
                                    .as_ref()
                                    .is_none_or(|(_, _, proposal)| approved.get() != Some(*proposal))
                            }) || !validation.get().is_empty()
                    })}
                    {..}
                    id="send-jira-create"
                    type="button"
                    on:click={create}
                >
                    {move || i18n.text("message.confirm_creation_in_jira")}
                </crate::button::Button>
                <crate::button::Button
                    {..}
                    id="cancel-jira-preparation"
                    type="button"
                    on:click={move |_| {
                        project.set(String::new());
                        types.set(Vec::new());
                        selected.set(String::new());
                        prepared.set(None);
                        approved.set(None);
                    }}
                >
                    {move || i18n.text("message.cancel_preparation")}
                </crate::button::Button>
            </fieldset>
            <fieldset
                hidden={move || !uncertain.get()}
                disabled={move || pending.get() || loading.get() || blocked.get()}
            >
                <p>
                    {move || {
                        i18n.text("message.check_the_project_in_jira_if_the_issue_exists_enter_its_key_if_a")
                    }}
                </p>
                <crate::form::Field
                    id="jira-created-key"
                    label={Signal::derive(move || i18n.text("message.existing_issue_key"))}
                >
                    <crate::form::Input
                        {..}
                        id="jira-created-key"
                        maxlength="100"
                        placeholder="SYN-1"
                        prop:value={move || key.get()}
                        on:input={move |event| key.set(event_target_value(&event).to_ascii_uppercase())}
                    />
                </crate::form::Field>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        key.get().trim().is_empty()
                            && intent
                                .with(|intent| intent.as_ref().is_none_or(|intent| intent.result.is_none()))
                    })}
                    {..}
                    id="resolve-jira-creation"
                    type="button"
                    on:click={reconcile}
                >
                    {move || i18n.text("message.verify_and_link_without_creating_again")}
                </crate::button::Button>
                <p class="field-help">
                    {move || {
                        i18n.text("message.closing_uncertainty_does_not_delete_any_remote_issue_creating_ag")
                    }}
                </p>
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="close-jira-creation-ack"
                        prop:checked={move || {
                            intent
                                .with(|intent| {
                                    intent.as_ref().is_some_and(|intent| intent.id == close_ack.get())
                                })
                        }}
                        on:change={move |event| {
                            close_ack
                                .set(
                                    if event_target_checked(&event) {
                                        intent
                                            .with(|intent| {
                                                intent
                                                    .as_ref()
                                                    .map(|intent| intent.id.clone())
                                                    .unwrap_or_default()
                                            })
                                    } else {
                                        String::new()
                                    },
                                )
                        }}
                    />
                    {move || {
                        i18n.text("message.i_checked_jira_and_want_to_close_this_uncertainty_without_sendin")
                    }}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        intent
                            .with(|intent| intent.as_ref().is_none_or(|intent| intent.id != close_ack.get()))
                    })}
                    {..}
                    id="close-jira-creation"
                    type="button"
                    on:click={close_uncertainty}
                >
                    {move || i18n.text("message.close_uncertainty_after_verification")}
                </crate::button::Button>
            </fieldset>
            <p class="field-help" hidden={move || !loading.get()}>
                {move || i18n.text("message.reading_jira_metadata")}
            </p>
            <p id="jira-create-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="jira-create-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
        </section>
    }
}
