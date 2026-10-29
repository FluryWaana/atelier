use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::contracts::{
    Card, JiraConfiguration, JiraReconciliation, JiraTransitionIntent, JiraTransitionRequest,
    JiraTransitions, JiraWriteState as Phase,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn JiraTask(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
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
                .get::<JiraTransitionIntent>("jiraTransition")
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
    let unresolved = Memo::new(move |_| {
        intent.with(|intent| {
            intent.as_ref().is_some_and(|intent| {
                matches!(
                    intent.state,
                    Phase::Preparing | Phase::Sending | Phase::Uncertain
                )
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
    let choices = RwSignal::new(None::<(Card, JiraTransitions)>);
    let selected = RwSignal::new(String::new());
    let confirm = RwSignal::new(false);
    let dismiss = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let mapping_context = Memo::new(move |_| {
        (
            serde_json::to_string(&board.settings.get()).unwrap_or_default(),
            serde_json::to_string(&board.projects.get()).unwrap_or_default(),
        )
    });
    Effect::new(move |_| {
        let _ = mapping_context.get();
        confirm.set(false);
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let reading_alive = alive.clone();
    let load = move |_| {
        if loading.get_untracked()
            || pending.get_untracked()
            || blocked.get_untracked()
            || unresolved.get_untracked()
        {
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
        loading.set(true);
        choices.set(None);
        confirm.set(false);
        selected.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = reading_alive.clone();
        let revision = revision();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!(
                        "/api/cards/{}/jira/transitions?revision={revision}",
                        expected.id
                    ),
                )
                .await?
                .json::<JiraTransitions>()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(value) => {
                        let settings = serde_json::to_value(board.settings.get_untracked())
                            .unwrap_or_default();
                        let projects = board
                            .projects
                            .get_untracked()
                            .iter()
                            .filter_map(|project| serde_json::to_value(project).ok())
                            .collect::<Vec<_>>();
                        let settings = atelier_core::domain::projects::settings_for_project(
                            &settings,
                            &projects,
                            &field(&expected.fields, "repo"),
                            settings["platform"].as_str() == Some("win32"),
                        );
                        if let Some(transition) =
                            atelier_core::application::projects::mapped_jira_transition(
                                &settings,
                                &field(&expected.fields, "status"),
                                &value.transitions,
                            )
                        {
                            selected.set(transition.to_owned());
                        }
                        choices.set(Some((expected, value)));
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let send_alive = alive.clone();
    let send = move |_| {
        if pending.get_untracked() || blocked.get_untracked() || unresolved.get_untracked() {
            return;
        }
        let Some((expected, options)) = choices.get_untracked() else {
            return;
        };
        let Some(transition) = options
            .transitions
            .iter()
            .find(|transition| {
                transition.id == selected.get_untracked() && !transition.requires_fields
            })
            .cloned()
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
        let body = serde_json::to_value(JiraTransitionRequest {
            revision: options.connection_revision,
            expected: atelier_core::application::jira::card_snapshot(&expected),
            from: options.current,
            transition,
        })
        .expect("Jira transition serialization");
        pending.set(true);
        confirm.set(false);
        error.set(String::new());
        notice.set(String::new());
        let alive = send_alive.clone();
        let path = format!("/api/cards/{}/jira/transition", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                choices.set(None);
                match result {
                    Ok(card) => {
                        let phase = card
                            .fields
                            .get::<JiraTransitionIntent>("jiraTransition")
                            .ok()
                            .flatten()
                            .map(|intent| intent.state);
                        notice.set(
                            if phase == Some(Phase::Confirmed) {
                                "message.jira_transition_confirmed_by_reading_the_status_again"
                            } else {
                                "message.jira_submission_is_uncertain_check_the_issue_before_continuing"
                            }
                            .into(),
                        );
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let reconcile_alive = alive.clone();
    let reconcile = StoredValue::new_local(move |dismiss_operation: bool| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        if dismiss_operation
            && expected
                .fields
                .get::<JiraTransitionIntent>("jiraTransition")
                .ok()
                .flatten()
                .is_none_or(|intent| intent.id != dismiss.get_untracked())
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
        let body = serde_json::to_value(JiraReconciliation {
            revision: revision(),
            expected: atelier_core::application::jira::card_snapshot(&expected),
            dismiss: dismiss_operation,
        })
        .expect("Jira reconciliation serialization");
        pending.set(true);
        dismiss.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = reconcile_alive.clone();
        let path = format!("/api/cards/{}/jira/reconcile", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(card) => {
                        let phase = card
                            .fields
                            .get::<JiraTransitionIntent>("jiraTransition")
                            .ok()
                            .flatten()
                            .map(|intent| intent.state);
                        notice.set(match phase{Some(Phase::Confirmed)=>"message.target_status_observed_nothing_was_resubmitted",Some(Phase::Dismissed)=>"message.uncertainty_closed_after_verification_nothing_was_resubmitted",_=>"message.the_target_status_is_not_confirmed_check_jira_before_closing_the"}.into());
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    view! {<section hidden=move ||!linked.get() class="task-files" aria-labelledby="jira-task-heading" aria-busy=move ||(pending.get()||loading.get()).to_string()>
        <h3 id="jira-task-heading">{move ||i18n.text("message.jira_issue_status")}</h3>
        <p class="field-help">{move ||i18n.text("message.this_action_changes_the_remote_issue_the_card_status_remains_ind")}</p>
        <p role="alert" hidden=move ||!unresolved.get()>{move ||i18n.text("message.jira_submission_is_in_progress_or_uncertain_incompatible_operati")}</p>
        <p>{move ||intent.get().and_then(|intent|intent.observed).map(|status|format!("{} · {}",i18n.text("message.last_observed_status"),status.name)).unwrap_or_default()}</p>
        <button id="load-jira-transitions" type="button" disabled=move ||pending.get()||loading.get()||blocked.get()||unresolved.get() on:click=load>{move ||i18n.text("message.load_available_transitions")}</button>
        <p role="status" hidden=move ||!loading.get()>{move ||i18n.text("message.reading_jira_transitions")}</p>
        <p hidden=move ||!choices.with(|choices|choices.as_ref().is_some_and(|(_,value)|value.transitions.is_empty()))>{move ||i18n.text("message.no_jira_transitions_are_available_for_this_account")}</p>
        <div hidden=move ||choices.with(|choices|choices.is_none())><label for="jira-transition">{move ||i18n.text("message.transition_to_submit")}</label><select id="jira-transition" disabled=move ||pending.get()||blocked.get()||unresolved.get() prop:value=move ||selected.get() on:change=move |event|{selected.set(event_target_value(&event));confirm.set(false);}>
            <option value="">{move ||i18n.text("message.choose_a_transition")}</option><For each=move ||choices.get().map(|(_,value)|value.transitions).unwrap_or_default() key=|transition|transition.id.clone() children=move |transition|view!{<option value=transition.id disabled=transition.requires_fields>{transition.name}{i18n.text("message.to_separator")}{transition.target.name}{if transition.requires_fields{" *"}else{""}}</option>} />
        </select><p class="field-help">{move ||i18n.text("message.transitions_requiring_additional_fields_must_be_performed_in_jir")}</p><button type="button" disabled=move ||selected.get().is_empty()||pending.get()||blocked.get()||unresolved.get() on:click=move |_|confirm.set(true)>{move ||i18n.text("message.prepare_the_jira_change")}</button></div>
        <div class="confirmation" role="alert" hidden=move ||!confirm.get()><p>{move ||i18n.text("message.confirm_submitting_this_transition_to_the_jira_issue")}</p><p>{move ||choices.get().and_then(|(_,value)|value.transitions.into_iter().find(|transition|transition.id==selected.get())).map(|transition|format!("{}{}{}",transition.name,i18n.text("message.to_separator"),transition.target.name)).unwrap_or_default()}</p><button id="confirm-jira-transition" type="button" disabled=move ||pending.get()||blocked.get()||unresolved.get() on:click=send>{move ||i18n.text("message.confirm_submission_to_jira")}</button><button type="button" on:click=move |_|confirm.set(false)>{move ||i18n.text("message.cancel")}</button></div>
        <div hidden=move ||intent.with(|intent|intent.as_ref().is_none_or(|intent|intent.state!=Phase::Uncertain))><button id="reconcile-jira-transition" type="button" disabled=move ||pending.get()||blocked.get() on:click=move |_|reconcile.with_value(|reconcile|reconcile(false))>{move ||i18n.text("message.check_the_status_without_resubmitting")}</button><label><input id="dismiss-jira-ack" type="checkbox" disabled=move ||pending.get() prop:checked=move ||intent.with(|intent|intent.as_ref().is_some_and(|intent|intent.id==dismiss.get())) on:change=move |event|dismiss.set(if event_target_checked(&event){intent.with(|intent|intent.as_ref().map(|intent|intent.id.clone()).unwrap_or_default())}else{String::new()}) />{move ||i18n.text("message.i_checked_the_issue_in_jira_and_want_to_close_this_uncertainty_w")}</label><button id="dismiss-jira-transition" type="button" disabled=move ||pending.get()||blocked.get()||intent.with(|intent|intent.as_ref().is_none_or(|intent|intent.id!=dismiss.get())) on:click=move |_|reconcile.with_value(|reconcile|reconcile(true))>{move ||i18n.text("message.close_uncertainty_after_verification")}</button></div>
        <p class="error" role="alert">{move ||i18n.text(&error.get())}</p><p role="status">{move ||i18n.text(&notice.get())}</p>
    </section>}
}
