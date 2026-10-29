use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::application::automation::{Flow, PublicationPlan, State};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Automation(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    on_applied: Callback<atelier_core::contracts::Card>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let flow = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<Flow>("automation").ok().flatten())
    });
    Effect::new(
        move |previous: Option<Option<(String, String, State, String)>>| {
            let version = flow.get().map(|flow| {
                (
                    flow.id,
                    flow.updated_at,
                    flow.state,
                    card.get()
                        .map(|card| field(&card.fields, "status"))
                        .unwrap_or_default(),
                )
            });
            if previous.as_ref() != Some(&version)
                && version.is_some()
                && let Some(current) = card.get()
            {
                on_applied.run(current);
            }
            version
        },
    );
    let project = Signal::derive(move || {
        card.get().and_then(|card| {
            let repo = field(&card.fields, "repo");
            board.projects.with(|projects| {
                projects
                    .iter()
                    .find(|project| field(project, "repo") == repo)
                    .cloned()
            })
        })
    });
    let profile = Signal::derive(move || {
        card.get().and_then(|card| {
            atelier_core::domain::agent_options::providers(&board.settings.get())
                .ok()
                .and_then(|profiles| {
                    profiles
                        .into_iter()
                        .find(|profile| field(profile, "id") == field(&card.fields, "agent"))
                })
        })
    });
    let snapshot = Memo::new(move |_| {
        serde_json::to_string(&(card.get(), project.get(), profile.get())).unwrap_or_default()
    });
    let approved = RwSignal::new(None::<String>);
    let branch_approved = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        let _ = snapshot.get();
        approved.set(None);
        branch_approved.set(None);
    });
    Effect::new(move |_| {
        if blocked.get() {
            approved.set(None);
            branch_approved.set(None);
        }
    });
    let publication = Signal::derive(move || {
        card.get().and_then(|card| {
            if let Ok(Some(preview)) = card
                .fields
                .get::<atelier_core::application::extensions::RepositoryProposal>(
                "connectorRepositoryPreview",
            ) {
                Some(PublicationPlan::Module(Box::new(preview)))
            } else if card.fields.raw("pullRequest").is_some() {
                card.fields
                    .get("gitPushPreview")
                    .ok()
                    .flatten()
                    .map(|preview| PublicationPlan::Push(Box::new(preview)))
            } else {
                card.fields
                    .get("forgePublicationPreview")
                    .ok()
                    .flatten()
                    .map(|preview| PublicationPlan::Create(Box::new(preview)))
            }
        })
    });
    let needs_branch = Memo::new(
        move |_| matches!(publication.get(),Some(PublicationPlan::Create(preview)) if preview.remote_source.is_none()),
    );
    let error = RwSignal::new(String::new());
    let active = Memo::new(move |_| flow.get().is_some_and(|flow| flow.state == State::Running));
    let action = Signal::derive(move || match flow.get().map(|flow| flow.state) {
        Some(State::Running) => None,
        Some(State::Waiting) => Some(("review", "message.check_publication_and_start_review")),
        Some(State::Interrupted | State::Error | State::Cancelled) => {
            Some(("resume", "message.resume_workflow_step"))
        }
        _ => Some(("start", "message.start_workflow")),
    });
    let execute = StoredValue::new_local(move |command: &str| {
        let stop = command == "stop";
        if pending.get_untracked()
            || (!stop
                && (blocked.get_untracked()
                    || approved.get_untracked().as_deref()
                        != Some(snapshot.get_untracked().as_str())))
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let path = format!(
            "/api/cards/{}/automation{}",
            expected.id,
            if stop { "/stop" } else { "" }
        );
        let body = if stop {
            serde_json::json!({"id":flow.get_untracked().map(|flow|flow.id),"confirmed":true})
        } else {
            let selected = if command == "publish" {
                "publish"
            } else if command == "restart" {
                "start"
            } else {
                let Some((selected, _)) = action.get_untracked() else {
                    return;
                };
                selected
            };
            serde_json::json!({"action":selected,"expected":expected,"project":project.get_untracked(),"profile":profile.get_untracked(),"allowCreateBranch":branch_approved.get_untracked().as_deref()==Some(snapshot.get_untracked().as_str()),"confirmed":true})
        };
        pending.set(true);
        approved.set(None);
        error.set(String::new());
        spawn_local(async move {
            let result = write(&credential, Method::POST, &path, &body).await;
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    });
    view! {<section class="task-files" id="automation" aria-labelledby="automation-heading" hidden=move||project.get().is_none()>
        <h3 id="automation-heading">{move||i18n.text("message.project_workflow")}</h3>
        <p class="field-help">{move||i18n.text("message.development_checks_and_review_use_the_saved_profile_p0_p1_correc")}</p>
        <p>{move||project.get().map(|project|format!("{} · {}",field(&project,"testCommand"),field(&project,"prTarget"))).unwrap_or_default()}</p>
        <p>{move||profile.get().map(|profile|field(&profile,"name")).unwrap_or_default()}</p>
        <p role="status">{move||flow.get().map(|flow|format!("{} · {} · {}",i18n.text(flow.state.translation_key()),i18n.text(flow.step.translation_key()),flow.cycles)).unwrap_or_default()}</p>
        <p role="status">{move||flow.get().map(|flow|i18n.text(&flow.message)).unwrap_or_default()}</p>
        <p class="field-help" hidden=move||project.get().is_some_and(|project|project.get::<bool>("automationEnabled").ok().flatten()==Some(true))>{move||i18n.text("message.enable_workflows_in_the_project_settings_to_start_a_task")}</p>
        <label hidden=move||active.get()><input id="approve-automation" type="checkbox" prop:checked=move||approved.get().as_deref()==Some(snapshot.get().as_str()) disabled=move||pending.get()||blocked.get() on:change=move|event|approved.set(event_target_checked(&event).then(||snapshot.get_untracked()))/>{move||i18n.text("message.authorize_this_step_with_the_displayed_requirements_profile_and")}</label>
        <button id="start-automation" type="button" hidden=move||active.get() disabled=move||pending.get()||blocked.get()||approved.get().as_deref()!=Some(snapshot.get().as_str()) on:click=move|_|execute.with_value(|execute|execute("step"))>{move||i18n.text(action.get().map(|(_,label)|label).unwrap_or("message.start_workflow"))}</button>
        <button id="restart-automation" type="button" hidden=move||flow.get().is_none_or(|flow|!matches!(flow.state,State::Interrupted|State::Error|State::Cancelled)) disabled=move||pending.get()||blocked.get()||approved.get().as_deref()!=Some(snapshot.get().as_str()) on:click=move|_|execute.with_value(|execute|execute("restart"))>{move||i18n.text("message.start_a_new_workflow_with_the_current_settings")}</button>
        <div hidden=move||flow.get().is_none_or(|flow|flow.state!=State::Waiting)||publication.get().is_none()>
            <h4>{move||i18n.text("message.run_the_approved_publication_workflow")}</h4>
            <p class="field-help">{move||i18n.text("message.first_prepare_the_commit_and_push_or_pr_mr_preview_in_the_the_ta")}</p>
            {move||publication.get().map(|plan| {let (repository,origin,branch,commit,title,body)=match plan {PublicationPlan::Create(preview)=>(preview.repository,preview.origin,format!("{}{}{}",preview.worktree.branch,i18n.text("message.to_separator"),preview.target),preview.source_commit,preview.title,preview.body),PublicationPlan::Push(preview)=>(preview.repository,preview.origin,preview.worktree.branch,preview.commit,String::new(),String::new()),PublicationPlan::Module(preview)=>(preview.observation.reference.repository,preview.context.origin,format!("{}{}{}",preview.context.worktree.branch,i18n.text("message.to_separator"),preview.observation.target_branch),preview.context.source,preview.title,preview.body)};view!{<p>{repository}" · "{origin}</p><p>{branch}</p><code>{commit}</code><p>{title}</p><p class="task-description">{body}</p>}})}
            <label hidden=move||!needs_branch.get()><input id="approve-workflow-branch" type="checkbox" disabled=move||pending.get()||blocked.get() prop:checked=move||branch_approved.get().as_deref()==Some(snapshot.get().as_str()) on:change=move|event|branch_approved.set(event_target_checked(&event).then(||snapshot.get_untracked()))/>{move||i18n.text("message.separately_authorize_creation_of_this_missing_remote_branch")}</label>
            <button id="publish-workflow" type="button" disabled=move||pending.get()||blocked.get()||approved.get().as_deref()!=Some(snapshot.get().as_str())||(needs_branch.get()&&branch_approved.get().as_deref()!=Some(snapshot.get().as_str())) on:click=move|_|execute.with_value(|execute|execute("publish"))>{move||i18n.text("message.recheck_publish_this_preview_and_start_review")}</button>
        </div>
        <button id="stop-automation" type="button" hidden=move||flow.get().is_none_or(|flow|!matches!(flow.state,State::Running|State::Waiting)) disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute("stop"))>{move||i18n.text("message.stop_workflow")}</button>
        <p id="automation-error" class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
