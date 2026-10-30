use crate::i18n::I18n;
use crate::{field, live::BoardState, write};
use atelier_core::application::projects::EDITABLE_FIELDS;
use atelier_core::document::Document;
use atelier_core::domain::agent_options::providers;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Default, PartialEq)]
struct ServiceDraft {
    repo: String,
    directory: String,
    command: String,
    port: String,
}

#[derive(Clone, Default, PartialEq)]
struct Draft {
    id: String,
    name: String,
    repo: String,
    rules: String,
    branch: String,
    agent: String,
    services: [ServiceDraft; 2],
    linked: String,
    verification: String,
    automation: bool,
    auto_fix: bool,
    target: String,
}
impl Draft {
    fn from_document(project: &Document) -> Self {
        Self {
            id: field(project, "id"),
            name: field(project, "name"),
            repo: field(project, "repo"),
            rules: field(project, "projectRules"),
            branch: field(project, "baseBranch"),
            agent: field(project, "agent"),
            services: ["front", "back"].map(|role| ServiceDraft {
                repo: field(project, &format!("{role}Repo")),
                directory: field(project, &format!("{role}Directory")),
                command: field(project, &format!("{role}Command")),
                port: project
                    .get::<u16>(&format!("{role}Port"))
                    .ok()
                    .flatten()
                    .unwrap_or(0)
                    .to_string(),
            }),
            linked: field(project, "linkedProjectId"),
            verification: field(project, "testCommand"),
            automation: project
                .get::<bool>("automationEnabled")
                .ok()
                .flatten()
                .unwrap_or(false),
            auto_fix: project
                .get::<bool>("autoFix")
                .ok()
                .flatten()
                .unwrap_or(true),
            target: field(project, "prTarget"),
        }
    }

    fn patch(&self, previous: &Self) -> serde_json::Value {
        let fields = |value: &Self| {
            let mut fields = serde_json::json!({"name":value.name, "repo":value.repo, "baseBranch":value.branch, "projectRules":value.rules, "agent":value.agent, "linkedProjectId":value.linked,"testCommand":value.verification,"automationEnabled":value.automation,"autoFix":value.auto_fix,"prTarget":value.target});
            for (index, role) in ["front", "back"].into_iter().enumerate() {
                let service = &value.services[index];
                for (suffix, text) in [
                    ("Repo", &service.repo),
                    ("Directory", &service.directory),
                    ("Command", &service.command),
                    ("Port", &service.port),
                ] {
                    fields[format!("{role}{suffix}")] =
                        serde_json::json!(if suffix == "Port" && text.is_empty() {
                            "0"
                        } else {
                            text.as_str()
                        });
                }
            }
            fields
        };
        let (next, previous) = (fields(self), fields(previous));
        let mut patch = serde_json::json!({"expected":{}});
        for field in EDITABLE_FIELDS {
            if next[field] != previous[field] {
                patch[field] = next[field].clone();
                patch["expected"][field] = previous[field].clone();
            }
        }
        patch
    }
}

#[component]
pub fn Projects(
    board: BoardState,
    repository: RwSignal<String>,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let projects = board.projects;
    let original = RwSignal::new(Draft::default());
    let draft = RwSignal::new(Draft::default());
    let access_dirty = RwSignal::new(false);
    let create_access_dirty = RwSignal::new(false);
    let front_access_dirty = RwSignal::new(false);
    let back_access_dirty = RwSignal::new(false);
    let dirty = Memo::new(move |_| {
        original.get() != draft.get()
            || access_dirty.get()
            || create_access_dirty.get()
            || front_access_dirty.get()
            || back_access_dirty.get()
    });
    let repository_locked = Memo::new(move |_| {
        let previous = original.get();
        !previous.id.is_empty()
            && board.cards.with(|cards| {
                cards.iter().any(|card| {
                    field(&card.fields, "repo") == previous.repo
                        && card
                            .fields
                            .get::<Document>("worktree")
                            .ok()
                            .flatten()
                            .is_some()
                })
            })
    });
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let refresh_failed = RwSignal::new(false);
    let error_text = move || {
        let message = i18n.text(&error.get());
        if refresh_failed.get() {
            i18n.text("message.project_saved_could_not_refresh_0")
                .replace("{0}", &message)
        } else {
            message
        }
    };
    let confirmation = RwSignal::new("");
    let archive = RwSignal::new(false);
    let editing = RwSignal::new(false);
    let creating = RwSignal::new(false);
    let navigation = StoredValue::new(None::<Callback<()>>);
    let close = move || {
        if let Some(next) = navigation.get_value() {
            navigation.set_value(None);
            next.run(());
        } else {
            on_close.run(());
        }
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            confirmation.set("discard");
            request_animation_frame(move || {
                if let Some(element) = document()
                    .get_element_by_id("discard-project-navigation")
                    .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    element.scroll_into_view();
                    let _ = element.focus();
                }
            });
        } else {
            close();
        }
    };
    let guard = expect_context::<crate::NavigationGuard>();
    guard.1.update(|generation| *generation += 1);
    let generation = guard.1.get_untracked();
    guard.0.set(Some(Callback::new(move |next: Callback<()>| {
        if pending.get_untracked() {
            return;
        }
        navigation.set_value(Some(next));
        request_close();
    })));
    on_cleanup(move || {
        if guard.1.get_untracked() == generation {
            guard.0.set(None);
        }
    });
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let select = move |value: Draft| {
        original.set(value.clone());
        draft.set(value);
        confirmation.set("");
        error.set(String::new());
        refresh_failed.set(false);
        archive.set(false);
    };
    let close_settings = move || {
        let target = if creating.get_untracked() {
            "add-project".to_owned()
        } else {
            format!("project-settings-{}", draft.get_untracked().id)
        };
        editing.set(false);
        creating.set(false);
        select(Draft::default());
        request_animation_frame(move || {
            if let Some(element) = document()
                .get_element_by_id(&target)
                .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.focus();
            }
        });
    };
    let request_settings_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            confirmation.set("settings-discard");
        } else {
            close_settings();
        }
    };
    let browse = move || {
        if pending.get_untracked() {
            return;
        }
        refresh_failed.set(false);
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(
                    &credential,
                    Method::POST,
                    "/api/folders/choose",
                    &serde_json::json!({"confirmed":true}),
                )
                .await?;
                let chosen: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|_| "message.invalid_server_response")?;
                if let Some(path) = chosen["path"].as_str().filter(|path| !path.is_empty()) {
                    draft.update(|value| {
                        value.repo = path.into();
                        if value.name.trim().is_empty() {
                            value.name = path
                                .rsplit(['/', '\\'])
                                .find(|part| !part.is_empty())
                                .unwrap_or_default()
                                .into();
                        }
                    });
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(message) = result {
                error.set(message);
            }
            pending.set(false);
        });
    };
    let save = move |remove: bool| {
        if pending.get_untracked() {
            return;
        }
        refresh_failed.set(false);
        let token = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let value = draft.get_untracked();
        let previous = original.get_untracked();
        let (method, path) = if remove {
            (Method::DELETE, format!("/api/projects/{}", value.id))
        } else if value.id.is_empty() {
            (Method::POST, "/api/projects".to_owned())
        } else {
            (Method::PATCH, format!("/api/projects/{}", value.id))
        };
        let body = if remove {
            serde_json::json!({"archiveTasks":archive.get_untracked()})
        } else if !value.id.is_empty() {
            value.patch(&previous)
        } else {
            serde_json::json!({"name":value.name, "repo":value.repo})
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&token, method, &path, &body).await?;
                if remove {
                    projects.update(|projects| {
                        projects.retain(|project| field(project, "id") != value.id)
                    });
                    if repository.get_untracked() == previous.repo {
                        repository.set(String::new());
                    }
                    select(Draft::default());
                    editing.set(false);
                } else {
                    let project: Document = response
                        .json()
                        .await
                        .map_err(|_| "message.invalid_server_response")?;
                    let saved = Draft::from_document(&project);
                    projects.update(|projects| {
                        if let Some(current) = projects
                            .iter_mut()
                            .find(|project| field(project, "id") == saved.id)
                        {
                            *current = project;
                        } else {
                            projects.push(project);
                        }
                    });
                    repository.set(saved.repo.clone());
                    if value.id.is_empty() {
                        select(Draft::default());
                        creating.set(false);
                    } else {
                        select(saved);
                    }
                }
                // A failed refresh must not turn an already committed creation into a retry.
                board.refresh(&token).await.map_err(|message| {
                    refresh_failed.set(true);
                    message
                })?;
                Ok::<(), String>(())
            }
            .await;
            if let Err(message) = result {
                let _ = board.refresh(&token).await;
                error.set(message);
            }
            pending.set(false);
        });
    };
    view! {
        <section id="projects-page" class="projects-page" aria-labelledby="workspace-title">
            <div class="projects-page-actions"><button id="close-projects" type="button" disabled=move ||pending.get() on:click=move |_|request_close()>{move ||i18n.text("message.back")}</button><button id="add-project" class="primary" type="button" disabled=move ||pending.get()||dirty.get() on:click=move |_|{select(Draft::default());editing.set(false);creating.set(true);}>{move ||i18n.text("message.add_a_project")}</button></div>
            <p class="error" role="alert" hidden=move ||editing.get()||creating.get()>{error_text}</p>
            <div class="projects-workspace" class:project-detail-open=move ||editing.get()>
            <section class="project-catalog" aria-label=move ||i18n.text("message.saved_projects")>
                <h2>{move ||i18n.text("message.your_projects")}</h2>
                <div id="project-list"><For each=move ||projects.get() key=|project|field(project,"id") children=move |project|{
                    let id=StoredValue::new(field(&project,"id"));
                    let current=move ||projects.with(|items|items.iter().find(|project|field(project,"id")==id.get_value()).cloned());
                    view!{<button class="project-choice" data-project-id=id.get_value() id=format!("project-settings-{}",id.get_value()) type="button" aria-pressed=move ||(draft.get().id==id.get_value()).to_string() disabled=move ||pending.get()||dirty.get() on:click=move |_|{if let Some(project)=current(){select(Draft::from_document(&project));editing.set(true);request_animation_frame(move ||{if let Some(element)=document().get_element_by_id("project-settings-title").and_then(|element|element.dyn_into::<web_sys::HtmlElement>().ok()){let _=element.focus();}});}}><strong>{move ||current().map(|project|field(&project,"name")).unwrap_or_default()}</strong><small>{move ||current().map(|project|field(&project,"repo")).unwrap_or_default()}</small></button>}
                }/></div>
                <p class="hint" hidden=move ||!projects.get().is_empty()>{move ||i18n.text("message.no_saved_projects")}</p>
            </section>
            <section id="project-settings-panel" class="settings-content project-detail" hidden=move ||!editing.get() aria-labelledby="project-settings-title">
                <header class="project-detail-heading"><div><h2 id="project-settings-title" tabindex="-1">{move ||draft.get().name}</h2><p class="hint">{move ||i18n.text("message.project_settings")}</p></div><button type="button" disabled=move ||pending.get() on:click=move |_|request_settings_close()>{move ||i18n.text("message.back")}</button></header><button class="project-open-tasks" type="button" disabled=move ||pending.get()||dirty.get() on:click=move |_|{repository.set(draft.get_untracked().repo);close();}>{move ||i18n.text("message.task_board")}</button>
            <form id="project-settings-form" on:submit=move |event| { event.prevent_default(); save(false); }>
                <fieldset disabled=move || pending.get()>
                    <section class="form-section"><div class="section-heading"><h3>{move ||i18n.text("message.main_project")}</h3></div>
                    <label for="project-name">{move || i18n.text("message.project_name")}</label><input id="project-name" required maxlength="100" autofocus prop:value=move || draft.get().name on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event)) />
                    <label for="project-repo">{move || i18n.text("message.git_repository_location")}</label><div class="directory-picker"><input id="project-repo" required maxlength="2000" spellcheck="false" readonly=move ||repository_locked.get() prop:value=move || draft.get().repo on:input=move |event| draft.update(|draft| draft.repo = event_target_value(&event)) /><button id="browse-project-settings" type="button" disabled=move ||repository_locked.get() on:click=move |_|browse()>{move ||i18n.text("message.browse")}</button></div>
                    <p class="hint">{move || i18n.text(if repository_locked.get(){"message.remove_this_projects_worktrees_before_changing_its_location"}else{"message.choose_an_existing_folder_no_files_will_be_moved_project_cards_w"})}</p>
                    <crate::repository_access::RepositoryAccess token pending dirty=access_dirty path=Signal::derive(move||if editing.get(){draft.get().repo}else{String::new()}) id="project-edit-access"/>
                    <label for="project-branch">{move || i18n.text("message.base_branch")}</label><input id="project-branch" maxlength="180" placeholder=move || i18n.text("message.default_value") prop:value=move || draft.get().branch on:input=move |event| draft.update(|draft| draft.branch = event_target_value(&event)) />
                    <label for="project-rules">{move || i18n.text("message.project_instructions")}</label><textarea id="project-rules" rows="4" maxlength="12000" prop:value=move || draft.get().rules on:input=move |event| draft.update(|draft| draft.rules = event_target_value(&event))></textarea>
                    <label for="project-agent">{move || i18n.text("message.preferred_agent")}</label><select id="project-agent" prop:value=move || draft.get().agent on:change=move |event| draft.update(|draft| draft.agent = event_target_value(&event))><option value="">{move || i18n.text("message.inherit_global_setting")}</option><For each=move || providers(&board.settings.get()).unwrap_or_default() key=|provider| field(provider,"id") children=move |provider| view! { <option value=field(&provider,"id")>{field(&provider,"name")}</option> } /></select>
                    </section><section class="form-section"><h3>{move ||i18n.text("message.application_launch")}</h3><ProjectServices draft=draft pending=pending token=token access_dirty=[front_access_dirty,back_access_dirty] /></section><section class="form-section">
                    <p class="field-help">{move||i18n.text("message.configure_jira_mapping_in_kanban")}</p>
                    </section><section class="form-section"><h3>{move ||i18n.text("message.automatic_development")}</h3><label for="project-test-command">{move||i18n.text("message.verification_command")}</label><textarea id="project-test-command" maxlength="2000" rows="2" spellcheck="false" prop:value=move||draft.get().verification on:input=move|event|draft.update(|draft|draft.verification=event_target_value(&event)) />
                    <p class="field-help">{move||i18n.text("message.this_command_runs_explicitly_in_the_task_worktree_with_your_acco")}</p>
                    <label for="project-pr-target">{move||i18n.text("message.workflow_target_branch")}</label><input id="project-pr-target" maxlength="180" spellcheck="false" prop:value=move||draft.get().target on:input=move|event|draft.update(|draft|draft.target=event_target_value(&event))/>
                    <label><input id="project-automation" type="checkbox" prop:checked=move||draft.get().automation on:change=move|event|draft.update(|draft|draft.automation=event_target_checked(&event))/>{move||i18n.text("message.allow_automatic_workflows_for_this_project")}</label>
                    <label><input id="project-auto-fix" type="checkbox" prop:checked=move||draft.get().auto_fix on:change=move|event|draft.update(|draft|draft.auto_fix=event_target_checked(&event))/>{move||i18n.text("message.correct_p0_p1_findings_up_to_two_cycles")}</label>
                    <p class="field-help">{move||i18n.text("message.the_workflow_starts_agents_and_checks_after_confirmation_on_the")}</p>
                    <label for="project-linked">{move || i18n.text("message.linked_project_for_testing")}</label>
                    <select id="project-linked" prop:value=move || draft.get().linked on:change=move |event| draft.update(|draft| draft.linked = event_target_value(&event))><option value="">{move || i18n.text("message.no_linked_project")}</option><For each={move || projects.get().into_iter().filter(|project| field(project,"id") != draft.get().id).collect::<Vec<_>>()} key=|project| field(project,"id") children=move |project| { let id = field(&project,"id"); let option_id=id.clone(); view! { <option value=option_id>{move || projects.with(|projects| projects.iter().find(|project| field(project,"id") == id).map(|project| field(project,"name")).unwrap_or_default())}</option> } } /></select>
                    <p class="field-help">{move || i18n.text("message.commands_and_the_linked_project_are_saved_for_future_launches_th")}</p></section>
                </fieldset>
                <p id="project-error" class="error" role="alert">{error_text}</p>
                <div class="form-actions"><button id="save-project" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "message.saving" } else { "message.save" })}</button><button id="reset-project" type="button" disabled=move || pending.get() || !dirty.get() on:click=move |_| confirmation.set("reset")>{move || i18n.text("message.discard_changes_8bc1ff70")}</button></div>
            </form>
            <div class="editor-secondary" hidden=move || draft.get().id.is_empty()><button id="remove-project" class="danger" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| confirmation.set("remove")>{move || i18n.text("message.remove_this_project")}</button></div>
            <div class="confirmation" role="alert" hidden=move || !matches!(confirmation.get(), "remove" | "reset" | "settings-discard")><p>{move || i18n.text(if confirmation.get() == "remove" { "message.remove_this_project_from_the_list_repository_files_and_tasks_wil" } else { "message.discard_unsaved_changes" })}</p>
                <label class="check-filter" hidden=move || confirmation.get() != "remove"><input id="archive-project-tasks" type="checkbox" prop:checked=move || archive.get() on:change=move |event| archive.set(event_target_checked(&event)) />{move || i18n.text("message.also_archive_this_project_s_tasks")}</label>
                <button id="confirm-project" class="danger" type="button" disabled=move || pending.get() on:click=move |_| match confirmation.get_untracked() { "remove" => save(true), "reset" => select(original.get_untracked()), "settings-discard"=>close_settings(), _ => close() }>{move || i18n.text("message.confirm")}</button><button id="cancel-project-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>{move || i18n.text("message.back")}</button>
            </div>
            </section>
            <p class="project-selection-hint hint" hidden=move ||editing.get()||projects.get().is_empty()>{move ||i18n.text("message.select_project_to_configure")}</p>
            </div>
            <div class="confirmation" role="alert" hidden=move ||confirmation.get()!="discard"><p>{move ||i18n.text("message.discard_unsaved_changes")}</p><button id="discard-project-navigation" type="button" class="danger" on:click=move |_|close()>{move ||i18n.text("message.discard")}</button><button type="button" on:click=move |_|{navigation.set_value(None);confirmation.set("");}>{move ||i18n.text("message.keep_editing")}</button></div>
        </section>
        <Show when=move ||creating.get()>
            <crate::modal::Modal id="project-create-dialog" labelledby="project-create-title" node_ref=NodeRef::new() on_close=Callback::new(move |()|{if !pending.get_untracked(){if dirty.get_untracked(){confirmation.set("create-discard");}else{close_settings();}}})>
                <header class="dialog-heading"><h2 id="project-create-title">{move ||i18n.text("message.add_a_project")}</h2><button type="button" data-close="" disabled=move ||pending.get() on:click=move |_|{if dirty.get_untracked(){confirmation.set("create-discard");}else{close_settings();}}>{move ||i18n.text("message.close")}</button></header>
            <form id="project-form" class="form-section" on:submit=move |event|{event.prevent_default();save(false);}>
                <div class="section-heading"><div><h3>{move ||i18n.text("message.add_a_project")}</h3><p class="hint">{move ||i18n.text("message.a_name_and_a_folder_to_find_your_tasks")}</p></div></div>
                <fieldset disabled=move ||pending.get()||editing.get()><label for="project-create-name">{move ||i18n.text("message.project_name")}</label><input id="project-create-name" required maxlength="100" placeholder=move ||i18n.text("message.my_application") prop:value=move ||draft.get().name on:input=move |event|draft.update(|draft|draft.name=event_target_value(&event))/>
                <label for="project-directory">{move ||i18n.text("message.local_directory")}</label><div class="directory-picker"><input id="project-directory" required maxlength="2000" prop:value=move ||draft.get().repo on:input=move |event|draft.update(|draft|draft.repo=event_target_value(&event))/><button id="browse-project" type="button" on:click=move |_|browse()>{move ||i18n.text("message.browse")}</button></div>
                <crate::repository_access::RepositoryAccess token pending dirty=create_access_dirty path=Signal::derive(move||if editing.get(){String::new()}else{draft.get().repo}) id="project-create-access"/>
                <p class="hint">{move ||i18n.text("message.choose_an_existing_folder_or_paste_its_path_worktrees_require_a")}</p><div class="form-actions"><button class="primary" type="submit" disabled=move ||!dirty.get()>{move ||i18n.text("message.add_project")}</button></div></fieldset>
                <p class="error" role="alert">{error_text}</p>
            </form>

                <div class="confirmation" role="alert" hidden=move ||confirmation.get()!="create-discard"><p>{move ||i18n.text("message.discard_unsaved_changes")}</p><button type="button" class="danger" on:click=move |_|close_settings()>{move ||i18n.text("message.discard")}</button><button type="button" on:click=move |_|confirmation.set("")>{move ||i18n.text("message.keep_editing")}</button></div>
            </crate::modal::Modal>
        </Show>

    }
}

#[component]
fn ProjectServices(
    access_dirty: [RwSignal<bool>; 2],
    draft: RwSignal<Draft>,
    pending: RwSignal<bool>,
    token: StoredValue<Result<String, String>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        <div class="row g-3 launch-settings-grid">
        {[(0usize, "front", "Front"), (1usize, "back", "Back")].into_iter().map(move |(index, role, label)| {
            let candidates = RwSignal::new(Vec::<Document>::new());
            let warnings = RwSignal::new(Vec::<String>::new());
            let error = RwSignal::new(String::new());
            let scanned = RwSignal::new(false);
            let identity = Memo::new(move |_| { let value = draft.get(); (value.id, value.repo, value.services[index].repo.clone()) });
            Effect::new(move |_| { let _ = identity.get(); candidates.set(Vec::new()); warnings.set(Vec::new()); error.set(String::new()); scanned.set(false); });
            let detect = move |_| {
                if pending.get_untracked() { return; }
                let value = draft.get_untracked();
                let credential = match token.get_value() { Ok(value) => value, Err(message) => { error.set(message); return; } };
                pending.set(true);
                candidates.set(Vec::new());
                warnings.set(Vec::new());
                error.set(String::new());
                scanned.set(false);
                spawn_local(async move {
                    let result = async {
                        let mut body = serde_json::json!({"role":role,"repo":value.repo});
                        body[format!("{role}Repo")] = serde_json::json!(value.services[index].repo);
                        let response = write(&credential, Method::POST, &format!("/api/projects/{}/detect-launch", value.id), &body).await?;
                        let result: Document = response.json().await.map_err(|_| "message.invalid_server_response".to_owned())?;
                        Ok::<_,String>((result.get::<Vec<Document>>("candidates").map_err(|_| "message.invalid_server_response")?.unwrap_or_default(), result.get::<Vec<String>>("warnings").map_err(|_| "message.invalid_server_response")?.unwrap_or_default()))
                    }.await;
                    match result {
                        Ok((found, notices)) => { candidates.set(found); warnings.set(notices); scanned.set(true); }
                        Err(message) => error.set(message),
                    }
                    pending.set(false);
                });
            };
            view! {
                <div class="col-12 col-md-6"><fieldset class="application-launch-panel" aria-labelledby=format!("project-{role}-heading")>
                    <legend id=format!("project-{role}-heading")>{label}</legend>
                    <label for=format!("project-{role}-repo")>{move || i18n.text("message.service_repository")}</label><input id=format!("project-{role}-repo") maxlength="2000" spellcheck="false" placeholder=move || i18n.text("message.main_repository_when_empty") prop:value=move || draft.get().services[index].repo.clone() on:input=move |event| draft.update(|draft| draft.services[index].repo = event_target_value(&event)) />
                    <crate::repository_access::RepositoryAccess token pending dirty=access_dirty[index] path=Signal::derive(move||draft.get().services[index].repo.clone()) id={if index==0 {"project-front-access"}else{"project-back-access"}}/>
                    <label for=format!("project-{role}-directory")>{move || i18n.text("message.service_subfolder")}</label><input id=format!("project-{role}-directory") maxlength="2000" spellcheck="false" placeholder=move || i18n.text("message.worktree_root_when_empty") prop:value=move || draft.get().services[index].directory.clone() on:input=move |event| draft.update(|draft| draft.services[index].directory = event_target_value(&event)) />
                    <label for=format!("project-{role}-command")>{move || i18n.text("message.launch_command")}</label><textarea id=format!("project-{role}-command") rows="2" maxlength="2000" spellcheck="false" prop:value=move || draft.get().services[index].command.clone() on:input=move |event| draft.update(|draft| draft.services[index].command = event_target_value(&event))></textarea>
                    <label for=format!("project-{role}-port")>{move || i18n.text("message.preferred_port")}</label><input id=format!("project-{role}-port") type="number" min="0" max="65535" step="1" prop:value=move || draft.get().services[index].port.clone() on:input=move |event| draft.update(|draft| draft.services[index].port = event_target_value(&event)) />
                    <p class="field-help">{move || i18n.text("message.0_or_empty_automatic_port_use_port_in_the_command_to_receive_the")}</p>
                    <button id=format!("detect-{role}") type="button" on:click=detect>{move || i18n.text("message.detect_commands")}</button>
                    <p class="field-help" hidden=move || !scanned.get() || !candidates.get().is_empty()>{move || i18n.text("message.no_command_detected_you_can_enter_your_own_command")}</p>
                    <div class="project-list"><For each={move || candidates.get().into_iter().enumerate().collect::<Vec<_>>()} key=|(index,_)| *index children=move |(_, candidate)| {
                        let directory = field(&candidate,"directory");
                        let command = field(&candidate,"command");
                        let runnable = !command.is_empty();
                        let text = format!("{} · {} · {}", field(&candidate,"technology"), if directory.is_empty() { "." } else { &directory }, command);
                        view! { <button class="launch-candidate" data-role=role type="button" disabled=!runnable on:click=move |_| { draft.update(|draft| { draft.services[index].directory = directory.clone(); draft.services[index].command = command.clone(); }); }>{text}</button> }
                    } /></div>
                    <div role="status">{move || warnings.get().into_iter().map(|warning| view! { <p>{i18n.text(&warning)}</p> }).collect_view()}</div>
                    <p class="error" role="alert">{move || i18n.text(&error.get())}</p>
                </fieldset></div>
            }
        }).collect_view()}
        </div>
    }
}
