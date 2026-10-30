use crate::{i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeWriteState, GitPushIntent, GitPushPreview, GitPushRequest,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn GitPush(
    card: Signal<Option<Card>>,
    configuration: Signal<Option<ForgeConnections>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    archived: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let approved = RwSignal::new(String::new());
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<GitPushPreview>("gitPushPreview")
                .ok()
                .flatten()
        })
    });
    let intent = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<GitPushIntent>("gitPush").ok().flatten())
    });
    let uncertain = Memo::new(move |_| {
        intent
            .get()
            .is_some_and(|intent| intent.state == ForgeWriteState::Uncertain)
    });
    let unresolved = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            atelier_core::application::forge::unresolved(&card.fields).unwrap_or(true)
        })
    });
    let key = Signal::derive(move || {
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card.get()?),"revision":configuration.get()?.revision}).to_string())
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked()
            || (action != "push-reconcile" && blocked.get_untracked())
            || (action != "push-reconcile" && (archived || unresolved.get_untracked()))
        {
            return;
        }
        if action == "push"
            && (preview.get_untracked().is_none()
                || key.get_untracked().as_deref() != Some(approved.get_untracked().as_str()))
        {
            return;
        }
        if action == "push-reconcile" && !uncertain.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!("/api/cards/{}/forge/{action}", expected.id);
        let body = serde_json::to_value(GitPushRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            confirmed: action != "push-preview",
        })
        .expect("push request serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
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
                        let unresolved = atelier_core::application::forge::unresolved(&card.fields)
                            .unwrap_or(true);
                        let diverged = card
                            .fields
                            .get::<GitPushIntent>("gitPush")
                            .ok()
                            .flatten()
                            .is_some_and(|intent| {
                                intent
                                    .observed
                                    .as_ref()
                                    .is_some_and(|observed| observed != &intent.preview.commit)
                            });
                        replace_card(board.cards, card);
                        notice.set(if action=="push-preview"{"message.preview_prepared_no_push_performed"}else if unresolved{"message.the_requested_commit_was_not_observed_the_push_remains_uncertain"}else if diverged{"message.the_remote_branch_changed_the_previous_push_result_remains_unkno"}else{"message.commit_observed_on_the_remote_branch_read_the_pr_mr_again_and_re"}.into());
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
    view! {
        <section
            class="task-files"
            hidden={move || {
                card.get().is_none_or(|card| card.fields.raw("pullRequest").is_none())
                    && intent.get().is_none()
            }}
            aria-labelledby="git-push-heading"
            aria-busy={move || pending.get().to_string()}
        >
            <h4 id="git-push-heading">{move || i18n.text("message.push_task_commits")}</h4>
            <p class="field-help">
                {move || {
                    i18n.text("message.only_existing_commits_will_be_sent_to_the_verified_task_branch_m")
                }}
            </p>
            <crate::button::Button
                disabled={Signal::derive(move || {
                    pending.get() || blocked.get() || archived || unresolved.get()
                        || configuration.with(|value| value.is_none())
                })}
                {..}
                id="preview-git-push"
                type="button"
                hidden={move || card.get().is_none_or(|card| card.fields.raw("pullRequest").is_none())}
                on:click={move |_| execute.with_value(|execute| execute("push-preview"))}
            >
                {move || i18n.text("message.prepare_push_preview")}
            </crate::button::Button>
            {move || {
                preview
                    .get()
                    .map(|preview| {
                        view! {
                            <dl>
                                <dt>{move || i18n.text("message.remote_branch")}</dt>
                                <dd>{preview.worktree.branch}</dd>
                                <dt>{move || i18n.text("message.expected_remote_commit")}</dt>
                                <dd>
                                    <code>
                                        {preview
                                            .previous
                                            .unwrap_or_else(|| i18n.text("message.branch_absent"))}
                                    </code>
                                </dd>
                                <dt>{move || i18n.text("message.commit_to_push")}</dt>
                                <dd>
                                    <code>{preview.commit}</code>
                                </dd>
                            </dl>
                            <p hidden={!preview
                                .local_changes}>
                                {move || {
                                    i18n.text(
                                        "message.local_files_have_changed_they_are_not_included_in_this_push",
                                    )
                                }}
                            </p>
                        }
                    })
            }}
            <fieldset
                hidden={move || preview.with(|preview| preview.is_none()) || unresolved.get()}
                disabled={move || pending.get() || blocked.get() || archived}
            >
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-git-push"
                        prop:checked={move || key.get().is_some_and(|key| key == approved.get())}
                        on:change={move |event| {
                            approved
                                .set(
                                    if event_target_checked(&event) {
                                        key.get_untracked().unwrap_or_default()
                                    } else {
                                        String::new()
                                    },
                                )
                        }}
                    />
                    {move || i18n.text("message.confirm_sending_this_commit_to_this_remote_branch")}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || key.get().is_none_or(|key| key != approved.get()))}
                    {..}
                    id="send-git-push"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute("push"))}
                >
                    {move || i18n.text("message.push_the_confirmed_commit")}
                </crate::button::Button>
            </fieldset>
            <crate::modal::Notice {..}  hidden={move || !uncertain.get()}>
                <p>
                    {move || i18n.text("message.push_uncertain_read_the_remote_branch_no_automatic_resend")}
                </p>
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get())}
                    {..}
                    id="reconcile-git-push"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute("push-reconcile"))}
                >
                    {move || i18n.text("message.read_remote_branch")}
                </crate::button::Button>
            </crate::modal::Notice>
            {move || {
                intent
                    .get()
                    .and_then(|intent| intent.observed)
                    .map(|observed| {
                        view! {
                            <p>
                                {move || i18n.text("message.last_observed_remote_commit")}" "
                                <code>{observed}</code>
                            </p>
                        }
                    })
            }}
            <p id="git-push-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="git-push-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
        </section>
    }
}
