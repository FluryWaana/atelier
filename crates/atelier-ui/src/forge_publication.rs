use crate::{i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeKind, ForgePublicationIntent, ForgePublicationPreview,
    ForgePublicationRequest, ForgeWriteState, PullRequest,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, PartialEq)]
struct Draft {
    kind: ForgeKind,
    repository: String,
    target: String,
    title: String,
    body: String,
    draft: bool,
}
impl From<ForgePublicationPreview> for Draft {
    fn from(value: ForgePublicationPreview) -> Self {
        Self {
            kind: value.configuration.kind,
            repository: value.repository,
            target: value.target,
            title: value.title,
            body: value.body,
            draft: value.draft,
        }
    }
}
#[component]
pub fn ForgePublication(
    card: Signal<Option<Card>>,
    configuration: Signal<Option<ForgeConnections>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
    archived: bool,
    on_linked: Callback<PullRequest>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgePublicationPreview>("forgePublicationPreview")
                .ok()
                .flatten()
        })
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgePublicationIntent>("forgePublication")
                .ok()
                .flatten()
        })
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
    let linked = Memo::new(move |_| {
        card.get()
            .is_some_and(|card| card.fields.raw("pullRequest").is_some())
    });
    let initial = preview
        .get_untracked()
        .or_else(|| intent.get_untracked().map(|intent| intent.preview))
        .map(Draft::from)
        .unwrap_or_else(|| Draft {
            kind: ForgeKind::Github,
            repository: String::new(),
            target: "main".into(),
            title: card
                .get_untracked()
                .and_then(|card| card.title.utf8().ok())
                .unwrap_or_default(),
            body: card
                .get_untracked()
                .and_then(|card| card.fields.get::<String>("description").ok().flatten())
                .unwrap_or_default(),
            draft: true,
        });
    let original = RwSignal::new(initial.clone());
    let draft = RwSignal::new(initial);
    let own_dirty = Memo::new(move |_| {
        let saved = preview
            .get()
            .or_else(|| intent.get().map(|intent| intent.preview))
            .map(Draft::from);
        // A prepared preview already persists these fields; only newer edits are unsaved.
        draft.get() != original.get() && saved.as_ref() != Some(&draft.get())
    });
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let approved = RwSignal::new(String::new());
    let push_approved = RwSignal::new(String::new());
    let needs_push = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            preview.remote_source.as_deref() != Some(preview.source_commit.as_str())
        })
    });
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let matches = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            configuration
                .get()
                .is_some_and(|configuration| configuration.revision == preview.revision)
                && Draft::from(preview) == draft.get()
        })
    });
    let key = Signal::derive(move || {
        let fields = draft.get();
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card.get()?),"revision":configuration.get()?.revision,"kind":fields.kind,"repository":fields.repository,"target":fields.target,"title":fields.title,"body":fields.body,"draft":fields.draft}).to_string())
    });
    Effect::new(move |previous: Option<Option<String>>| {
        let current = key.get();
        if previous.as_ref() != Some(&current) {
            approved.set(String::new());
            push_approved.set(String::new());
        }
        current
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked() || (action != "publication-reconcile" && blocked.get_untracked())
        {
            return;
        }
        if action == "publication-reconcile" {
            if !uncertain.get_untracked() {
                return;
            }
        } else if archived || linked.get_untracked() || unresolved.get_untracked() {
            return;
        }
        if action == "publication"
            && (needs_push.get_untracked()
                || !matches.get_untracked()
                || key.get_untracked().as_deref() != Some(approved.get_untracked().as_str()))
        {
            return;
        }
        if action == "publication-push"
            && (!needs_push.get_untracked()
                || !matches.get_untracked()
                || key.get_untracked().as_deref() != Some(push_approved.get_untracked().as_str()))
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        let values = draft.get_untracked();
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!("/api/cards/{}/forge/{action}", expected.id);
        let body = serde_json::to_value(ForgePublicationRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            kind: values.kind,
            repository: values.repository,
            target: values.target,
            title: values.title,
            body: values.body,
            draft: values.draft,
            confirmed: action != "publication-preview",
        })
        .expect("publication request serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
        push_approved.set(String::new());
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
                        if action == "publication-preview"
                            && let Ok(Some(preview)) = card
                                .fields
                                .get::<ForgePublicationPreview>("forgePublicationPreview")
                        {
                            draft.set(Draft::from(preview));
                        }
                        let unresolved = atelier_core::application::forge::unresolved(&card.fields)
                            .unwrap_or(true);
                        if let Ok(Some(pr)) = card.fields.get::<PullRequest>("pullRequest") {
                            let published = card
                                .fields
                                .get::<ForgePublicationIntent>("forgePublication")
                                .ok()
                                .flatten()
                                .map(|intent| Draft::from(intent.preview));
                            if published.as_ref() == Some(&draft.get_untracked()) {
                                original.set(draft.get_untracked());
                            }
                            on_linked.run(pr);
                        }
                        replace_card(board.cards, card);
                        notice.set(if action=="publication-preview"{"message.preview_prepared_no_pr_mr_created"}else if action=="publication-push"{if unresolved{"message.push_uncertain_read_the_remote_branch_no_automatic_resend"}else{"message.remote_branch_read_again_confirm_pr_mr_creation_separately_once"}}else if unresolved{"message.publication_found_but_cannot_be_linked_uncertainty_is_retained"}else{"message.pr_mr_found_checked_and_linked_no_creation_was_resent"}.into());
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
    view! {<section class="task-files" hidden=move||linked.get()&&!own_dirty.get()&&!uncertain.get() aria-labelledby="forge-publication-heading" aria-busy=move||pending.get().to_string()><h4 id="forge-publication-heading">{move||i18n.text("message.create_a_pr_mr")}</h4>
        <p class="field-help">{move||i18n.text("message.preview_the_branches_and_content_if_needed_confirm_the_branch_pu")}</p>
        <form on:submit=move|event:leptos::ev::SubmitEvent|{event.prevent_default();execute.with_value(|execute|execute("publication-preview"));}><fieldset disabled=move||pending.get()||blocked.get()||archived||linked.get()||unresolved.get()||configuration.with(|value|value.is_none())>
            <label for="publication-kind">{move||i18n.text("message.forge")}</label><select id="publication-kind" prop:value=move||draft.get().kind.name() on:change=move|event|draft.update(|draft|draft.kind=match event_target_value(&event).as_str(){"gitlab"=>ForgeKind::Gitlab,"azure"=>ForgeKind::Azure,_=>ForgeKind::Github})><option value="github">"GitHub"</option><option value="gitlab">"GitLab"</option><option value="azure">"Azure DevOps Services"</option></select>
            <label for="publication-repository">{move||i18n.text("message.pr_mr_repository")}</label><input id="publication-repository" required maxlength="1000" prop:value=move||draft.get().repository on:input=move|event|draft.update(|draft|draft.repository=event_target_value(&event)) />
            <label for="publication-target">{move||i18n.text("message.target_branch")}</label><input id="publication-target" required maxlength="160" prop:value=move||draft.get().target on:input=move|event|draft.update(|draft|draft.target=event_target_value(&event)) />
            <label for="publication-title">{move||i18n.text("message.pr_mr_title")}</label><input id="publication-title" required maxlength="255" prop:value=move||draft.get().title on:input=move|event|draft.update(|draft|draft.title=event_target_value(&event)) />
            <label for="publication-body">{move||i18n.text("message.pr_mr_description")}</label><textarea id="publication-body" maxlength="3500" prop:value=move||draft.get().body on:input=move|event|draft.update(|draft|draft.body=event_target_value(&event)) />
            <label><input id="publication-draft" type="checkbox" prop:checked=move||draft.get().draft on:change=move|event|draft.update(|draft|draft.draft=event_target_checked(&event)) />{move||i18n.text("message.create_as_draft")}</label>
            <button id="preview-publication" type="submit">{move||i18n.text("message.prepare_publication_preview")}</button>
        </fieldset></form>
        {move||preview.get().map(|preview|view!{<p>{preview.worktree.branch}{i18n.text("message.to_separator")}{preview.target}</p><p><code>{preview.source_commit}</code>" / "<code>{preview.target_commit}</code></p><p>{move||i18n.text("message.expected_remote_commit")}" : "<code>{preview.remote_source.unwrap_or_else(||i18n.text("message.branch_absent"))}</code></p><p hidden=!preview.local_changes>{move||i18n.text("message.local_files_changed_they_are_not_included_in_this_publication")}</p>})}
        <fieldset hidden=move||!matches.get()||!needs_push.get()||unresolved.get()||linked.get() disabled=move||pending.get()||blocked.get()||archived><p>{move||i18n.text("message.pushing_writes_to_the_forge_and_may_trigger_its_ci_rules_an_abse")}</p><label><input id="confirm-publication-push" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==push_approved.get()) on:change=move|event|push_approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()}) />{move||i18n.text("message.confirm_sending_this_commit_to_this_remote_branch")}</label><button id="push-publication-branch" type="button" disabled=move||key.get().is_none_or(|key|key!=push_approved.get()) on:click=move|_|execute.with_value(|execute|execute("publication-push"))>{move||i18n.text("message.push_branch_before_publication")}</button></fieldset>
        <fieldset hidden=move||!matches.get()||needs_push.get()||unresolved.get()||linked.get() disabled=move||pending.get()||blocked.get()||archived><p>{move||i18n.text("message.creation_writes_to_the_forge_and_may_trigger_its_notifications_o")}</p><label><input id="confirm-publication" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()}) />{move||i18n.text("message.confirm_creating_this_pr_mr_with_these_branches_and_content")}</label><button id="create-publication" type="button" disabled=move||key.get().is_none_or(|key|key!=approved.get()) on:click=move|_|execute.with_value(|execute|execute("publication"))>{move||i18n.text("message.create_the_confirmed_pr_mr")}</button></fieldset>
        <div class="confirmation" hidden=move||!uncertain.get()><p>{move||i18n.text("message.publication_uncertain_search_for_the_existing_pr_mr_no_automatic")}</p><button id="reconcile-publication" type="button" disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute("publication-reconcile"))>{move||i18n.text("message.find_existing_publication")}</button></div>
        <button id="cancel-publication" type="button" hidden=move||!own_dirty.get() disabled=move||pending.get()||unresolved.get() on:click=move|_|{draft.set(original.get_untracked());approved.set(String::new());}>{move||i18n.text("message.cancel_preparation")}</button>
        <p id="publication-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="publication-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
