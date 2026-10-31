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
struct CloneDraft {
    enabled: bool,
    url: String,
    parent: String,
    directory: String,
    connection: String,
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
    let clone_draft = RwSignal::new(CloneDraft::default());
    let clone_operation = RwSignal::new(String::new());
    let cloned = RwSignal::new(false);
    let access_dirty = RwSignal::new(false);
    let create_access_dirty = RwSignal::new(false);
    let front_access_dirty = RwSignal::new(false);
    let back_access_dirty = RwSignal::new(false);
    let automatic_branch = RwSignal::new(None::<String>);
    let dirty = Memo::new(move |_| {
        let saved = original.get();
        let mut current = draft.get();
        // A detected default is a suggestion, not a user edit.
        if saved.branch.is_empty()
            && automatic_branch.get().as_deref() == Some(current.branch.as_str())
        {
            current.branch.clear();
        }
        saved != current
            || clone_draft.get() != CloneDraft::default()
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
    let branch_loading = RwSignal::new(false);
    let branch_error = RwSignal::new(String::new());
    let branch_request = RwSignal::new(0u64);
    let detect_branch = Callback::new(move |repo: String| {
        let selected = draft.get_untracked();
        if selected.id.is_empty()
            || selected.repo != repo
            || !selected.branch.trim().is_empty()
            || branch_loading.get_untracked()
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(_) => return,
        };
        branch_request.update(|value| *value += 1);
        let request = branch_request.get_untracked();
        branch_loading.set(true);
        branch_error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(
                    &credential,
                    Method::POST,
                    "/api/repositories/default-branch",
                    &serde_json::json!({"path":repo}),
                )
                .await?;
                let value: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                Ok::<_, String>(
                    value["branch"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                )
            }
            .await;
            if branch_request.try_get_untracked() != Some(request) {
                return;
            }
            branch_loading.set(false);
            let current = draft.get_untracked();
            if current.id != selected.id
                || current.repo != repo
                || !current.branch.trim().is_empty()
            {
                return;
            }
            match result {
                Ok(Some(branch)) => {
                    automatic_branch.set(Some(branch.clone()));
                    draft.update(|draft| draft.branch = branch);
                }
                Ok(None) => branch_error.set("message.project_branch_not_detected".into()),
                Err(_) => branch_error.set("message.project_branch_read_failed".into()),
            }
        });
    });
    let error_text = move || {
        let message = i18n.text(&error.get());
        if refresh_failed.get() {
            i18n.text("message.project_saved_could_not_refresh_0")
                .replace("{0}", &message)
        } else {
            message
        }
    };
    let navigation = StoredValue::new(None::<Callback<()>>);
    let confirmation = RwSignal::new("");
    let confirmation_name = RwSignal::new(String::new());
    let confirmation_focus = StoredValue::new(String::new());
    let ask_confirmation = move |action| {
        confirmation_focus.set_value(
            document()
                .active_element()
                .map(|element| element.id())
                .unwrap_or_default(),
        );
        confirmation_name.set(String::new());
        confirmation.set(action);
    };
    let dismiss_confirmation = move || {
        navigation.set_value(None);
        confirmation.set("");
        let target = confirmation_focus.get_value();
        request_animation_frame(move || {
            if let Some(element) = document()
                .get_element_by_id(&target)
                .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.focus();
            }
        });
    };
    let archive = RwSignal::new(false);
    let editing = RwSignal::new(false);
    let tab = RwSignal::new(0usize);
    const TABS: [(&str, &str); 4] = [
        ("general", "message.general"),
        ("agent", "message.agent"),
        ("launch", "message.project_launch_tab"),
        ("automation", "message.project_automation_tab"),
    ];
    let creating = RwSignal::new(false);
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
            ask_confirmation("discard");
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
        clone_draft.set(CloneDraft::default());
        cloned.set(false);
        automatic_branch.set(None);
        for flag in [
            access_dirty,
            create_access_dirty,
            front_access_dirty,
            back_access_dirty,
        ] {
            flag.set(false);
        }
        branch_request.update(|value| *value += 1);
        branch_loading.set(false);
        branch_error.set(String::new());
        if original.get_untracked().id != value.id {
            tab.set(0);
        }
        original.set(value.clone());
        draft.set(value);
        confirmation.set("");
        error.set(String::new());
        refresh_failed.set(false);
        archive.set(false);
    };
    let close_settings = move || {
        navigation.set_value(None);
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
            ask_confirmation("settings-discard");
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
                    if clone_draft.get_untracked().enabled {
                        clone_draft.update(|value| value.parent = path.into());
                        return Ok(());
                    }
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
        if pending.get_untracked() || branch_loading.get_untracked() {
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
        let clone = clone_draft.get_untracked();
        let previous = original.get_untracked();
        let (method, path) = if remove {
            (Method::DELETE, format!("/api/projects/{}", value.id))
        } else if value.id.is_empty() {
            (Method::POST, "/api/projects".to_owned())
        } else {
            (Method::PATCH, format!("/api/projects/{}", value.id))
        };
        let mut body = if remove {
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
                if clone.enabled && value.id.is_empty() && !remove {
                    let operation = format!("clone-{}", js_sys::Date::now() as u64);
                    clone_operation.set(operation.clone());
                    let response = write(&token, Method::POST, "/api/repositories/clone", &serde_json::json!({
                        "url":clone.url, "parent":clone.parent, "directory":clone.directory,
                        "connection":if clone.connection.is_empty() { None } else { Some(clone.connection) },
                        "operation":operation, "confirmed":true,
                    })).await;
                    clone_operation.set(String::new());
                    let result: serde_json::Value = response?.json().await.map_err(|_| "message.invalid_server_response")?;
                    let path = result["path"].as_str().filter(|path| !path.is_empty()).ok_or("message.invalid_server_response")?;
                    body["repo"] = serde_json::json!(path);
                    draft.update(|draft| draft.repo = path.into());
                    clone_draft.set(CloneDraft::default());
                    cloned.set(true);
                    if result["authorized"].as_bool() != Some(true) {
                        return Err("message.clone_authorization_failed".into());
                    }
                }
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
                        if clone.enabled {
                            select(saved);
                            editing.set(true);
                        } else {
                            select(Draft::default());
                        }
                        creating.set(false);
                    } else {
                        select(saved);
                    }
                }
                // A failed refresh must not turn an already committed creation into a retry.
                board.refresh(&token).await.inspect_err(|_message| {
                    refresh_failed.set(true);
                })?;
                Ok::<(), String>(())
            }
            .await;
            if let Err(message) = result {
                let _ = board.refresh(&token).await;
                error.set(message);
            }
            pending.set(false);
            if clone.enabled && editing.get_untracked() {
                detect_branch.run(draft.get_untracked().repo);
            }
            if remove && draft.get_untracked().id.is_empty() {
                request_animation_frame(move || {
                    if let Some(element) = document()
                        .get_element_by_id("add-project")
                        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        let _ = element.focus();
                    }
                });
            }
        });
    };
    view! {
        <section id="projects-page" class="projects-page" aria-labelledby="workspace-title">
            <div class="projects-page-actions">
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get())}
                    {..}
                    id="close-projects"
                    type="button"
                    on:click={move |_| {
                        if editing.get_untracked() {
                            request_settings_close();
                        } else {
                            request_close();
                        }
                    }}
                >
                    {move || {
                        i18n.text(
                            if editing.get() {
                                "message.project_back_to_list"
                            } else {
                                "message.project_back_to_overview"
                            },
                        )
                    }}
                </crate::button::Button>
                <crate::button::Button
                    variant={crate::button::ButtonVariant::Primary}
                    disabled={Signal::derive(move || pending.get())}
                    class="primary"
                    {..}
                    id="add-project"
                    type="button"
                    on:click={move |_| {
                        navigation
                            .set_value(
                                Some(
                                    Callback::new(move |()| {
                                        select(Draft::default());
                                        editing.set(false);
                                        creating.set(true);
                                    }),
                                ),
                            );
                        request_close();
                    }}
                >
                    {move || i18n.text("message.add_a_project")}
                </crate::button::Button>
            </div>
            <p class="error" role="alert" hidden={move || editing.get() || creating.get()}>
                {error_text}
            </p>
            <div class="projects-workspace" class:project-detail-open={move || editing.get()}>
                <section class="project-catalog" aria-label={move || i18n.text("message.saved_projects")}>
                    <h2>{move || i18n.text("message.saved_projects")}</h2>
                    <div id="project-list">
                        <For
                            each={move || projects.get()}
                            key={|project| field(project, "id")}
                            children={move |project| {
                                let id = StoredValue::new(field(&project, "id"));
                                let current = move || {
                                    projects
                                        .with(|items| {
                                            items
                                                .iter()
                                                .find(|project| field(project, "id") == id.get_value())
                                                .cloned()
                                        })
                                };
                                view! {
                                    <crate::button::Button
                                        disabled={Signal::derive(move || pending.get())}
                                        class="ui-card-choice project-choice"
                                        {..}
                                        data-project-id={id.get_value()}
                                        id={format!("project-settings-{}", id.get_value())}
                                        type="button"
                                        aria-pressed={move || (draft.get().id == id.get_value()).to_string()}
                                        on:click={move |_| {
                                            if editing.get_untracked()
                                                && draft.get_untracked().id == id.get_value()
                                            {
                                                return;
                                            }
                                            navigation
                                                .set_value(
                                                    Some(
                                                        Callback::new(move |()| {
                                                            if let Some(project) = current() {
                                                                select(Draft::from_document(&project));
                                                                editing.set(true);
                                                                request_animation_frame(move || {
                                                                    if let Some(element) = document()
                                                                        .get_element_by_id("project-settings-title")
                                                                        .and_then(|element| {
                                                                            element.dyn_into::<web_sys::HtmlElement>().ok()
                                                                        })
                                                                    {
                                                                        let _ = element.focus();
                                                                    }
                                                                });
                                                            }
                                                        }),
                                                    ),
                                                );
                                            request_close();
                                        }}
                                    >
                                        <strong>
                                            {move || {
                                                current()
                                                    .map(|project| field(&project, "name"))
                                                    .unwrap_or_default()
                                            }}
                                        </strong>
                                        <small>
                                            {move || {
                                                current()
                                                    .map(|project| field(&project, "repo"))
                                                    .unwrap_or_default()
                                            }}
                                        </small>
                                        <span class="project-choice-action">
                                            {move || i18n.text("message.project_configure")}
                                            <svg
                                                width="16"
                                                height="16"
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="1.7"
                                                aria-hidden="true"
                                            >
                                                <path d="m9 5 7 7-7 7" />
                                            </svg>
                                        </span>
                                    </crate::button::Button>
                                }
                            }}
                        />
                    </div>
                    <p class="hint" hidden={move || !projects.get().is_empty()}>
                        {move || i18n.text("message.no_saved_projects")}
                    </p>
                </section>
                <section
                    id="project-settings-panel"
                    class="settings-content project-detail"
                    hidden={move || !editing.get()}
                    aria-labelledby="project-settings-title"
                >
                    <header class="project-detail-heading">
                        <h2 id="project-settings-title" tabindex="-1">
                            {move || draft.get().name}
                        </h2>
                        <crate::button::Button
                            disabled={Signal::derive(move || pending.get())}
                            class="project-open-tasks"
                            {..}
                            type="button"
                            on:click={move |_| {
                                navigation
                                    .set_value(
                                        Some(
                                            Callback::new(move |()| {
                                                repository.set(original.get_untracked().repo);
                                                on_close.run(());
                                            }),
                                        ),
                                    );
                                request_close();
                            }}
                        >
                            {move || i18n.text("message.project_open_tasks")}
                        </crate::button::Button>
                    </header>
                    <nav
                        class="project-tabs"
                        role="tablist"
                        aria-label={move || i18n.text("message.settings_sections")}
                    >
                        {TABS
                            .into_iter()
                            .enumerate()
                            .map(move |(index, (id, label))| {
                                view! {
                                    <crate::button::Button
                                        disabled={Signal::derive(move || pending.get())}
                                        class="view-tab"
                                        {..}
                                        type="button"
                                        role="tab"
                                        class:selected-tab={move || tab.get() == index}
                                        id={format!("project-tab-{id}")}
                                        aria-controls={format!("project-panel-{id}")}
                                        aria-selected={move || (tab.get() == index).to_string()}
                                        tabindex={move || if tab.get() == index { "0" } else { "-1" }}
                                        on:click={move |_| tab.set(index)}
                                        on:keydown={move |event: web_sys::KeyboardEvent| {
                                            if pending.get_untracked() {
                                                return;
                                            }
                                            let next = match event.key().as_str() {
                                                "ArrowRight" => (index + 1) % TABS.len(),
                                                "ArrowLeft" => (index + TABS.len() - 1) % TABS.len(),
                                                "Home" => 0,
                                                "End" => TABS.len() - 1,
                                                _ => return,
                                            };
                                            event.prevent_default();
                                            tab.set(next);
                                            if let Some(element) = document()
                                                .get_element_by_id(&format!("project-tab-{}", TABS[next].0))
                                                .and_then(|element| {
                                                    element.dyn_into::<web_sys::HtmlElement>().ok()
                                                })
                                            {
                                                let _ = element.focus();
                                            }
                                        }}
                                    >
                                        {move || i18n.text(label)}
                                    </crate::button::Button>
                                }
                            })
                            .collect_view()}
                    </nav>
                    <crate::form::Form
                        busy={Signal::derive(move || pending.get())}
                        on_submit={Callback::new(move |_| save(false))}
                        on_invalid={Callback::new(move |invalid: web_sys::Element| {
                            if let Ok(Some(panel)) = invalid.closest("[role=tabpanel]")
                                && let Some(index) = TABS
                                    .iter()
                                    .position(|(id, _)| panel.id() == format!("project-panel-{id}"))
                                {
                                    tab.set(index);
                                }
                        })}
                        {..}
                        id="project-settings-form"
                    >
                        <fieldset disabled={move || pending.get()}>
                            <section
                                id="project-panel-general"
                                class="form-section"
                                role="tabpanel"
                                aria-labelledby="project-tab-general"
                                hidden={move || tab.get() != 0}
                            >
                                <crate::form::Field
                                    id="project-name"
                                    label={Signal::derive(move || i18n.text("message.project_name"))}
                                >
                                    <crate::form::Input
                                        {..}
                                        id="project-name"
                                        required=true
                                        maxlength="100"
                                        autofocus=true
                                        prop:value={move || draft.get().name}
                                        on:input={move |event| {
                                            draft.update(|draft| draft.name = event_target_value(&event))
                                        }}
                                    />
                                </crate::form::Field>
                                <crate::form::Field
                                    id="project-repo"
                                    label={Signal::derive(move || {
                                        i18n.text("message.git_repository_location")
                                    })}
                                >
                                    <div class="directory-picker">
                                        <crate::form::Input
                                            copy=true
                                            {..}
                                            id="project-repo"
                                            required=true
                                            maxlength="2000"
                                            spellcheck="false"
                                            readonly={move || repository_locked.get()}
                                            prop:value={move || draft.get().repo}
                                            on:input={move |event| {
                                                draft.update(|draft| draft.repo = event_target_value(&event))
                                            }}
                                        />
                                        <crate::button::Button
                                            disabled={Signal::derive(move || repository_locked.get())}
                                            {..}
                                            id="browse-project-settings"
                                            type="button"
                                            on:click={move |_| browse()}
                                        >
                                            {move || i18n.text("message.browse")}
                                        </crate::button::Button>
                                    </div>
                                </crate::form::Field>
                                <p class="hint">
                                    {move || {
                                        i18n.text(
                                            if repository_locked.get() {
                                                "message.remove_this_projects_worktrees_before_changing_its_location"
                                            } else {
                                                "message.choose_an_existing_folder_no_files_will_be_moved_project_cards_w"
                                            },
                                        )
                                    }}
                                </p>
                                <crate::repository_access::RepositoryAccess
                                    token
                                    pending
                                    dirty={access_dirty}
                                    path={Signal::derive(move || {
                                        if editing.get() { draft.get().repo } else { String::new() }
                                    })}
                                    id="project-edit-access"
                                    on_authorized={detect_branch}
                                />
                                <crate::form::Field
                                    id="project-branch"
                                    label={Signal::derive(move || i18n.text("message.base_branch"))}
                                    help={Signal::derive(move || {
                                        i18n.text(
                                            if branch_loading.get() {
                                                "message.project_branch_loading"
                                            } else {
                                                "message.project_branch_help"
                                            },
                                        )
                                    })}
                                    error={Signal::derive(move || i18n.text(&branch_error.get()))}
                                >
                                    <crate::form::Input
                                        {..}
                                        id="project-branch"
                                        maxlength="180"
                                        placeholder={move || i18n.text("message.project_branch_automatic")}

                                        prop:value={move || draft.get().branch}
                                        on:input={move |event| {
                                            draft.update(|draft| draft.branch = event_target_value(&event))
                                        }}
                                    />
                                </crate::form::Field>
                                <crate::button::Button
                                    disabled={Signal::derive(move || pending.get() || branch_loading.get())}
                                    {..}
                                    type="button"
                                    hidden={move || branch_error.get().is_empty()}
                                    on:click={move |_| detect_branch.run(draft.get_untracked().repo)}
                                >
                                    {move || i18n.text("message.project_retry")}
                                </crate::button::Button>
                            </section>
                            <section
                                id="project-panel-agent"
                                class="form-section"
                                role="tabpanel"
                                aria-labelledby="project-tab-agent"
                                hidden={move || tab.get() != 1}
                            >
                                <crate::form::Field
                                    id="project-rules"
                                    label={Signal::derive(move || i18n.text("message.project_instructions"))}
                                >
                                    <crate::form::Textarea
                                        {..}
                                        id="project-rules"
                                        rows="4"
                                        maxlength="12000"
                                        prop:value={move || draft.get().rules}
                                        on:input={move |event| {
                                            draft.update(|draft| draft.rules = event_target_value(&event))
                                        }}
                                    ></crate::form::Textarea>
                                </crate::form::Field>
                                <crate::form::Field
                                    id="project-agent"
                                    label={Signal::derive(move || i18n.text("message.preferred_agent"))}
                                >
                                    <crate::form::Select
                                        {..}
                                        id="project-agent"
                                        prop:value={move || draft.get().agent}
                                        on:change={move |event| {
                                            draft.update(|draft| draft.agent = event_target_value(&event))
                                        }}
                                    >
                                        <option value="">
                                            {move || i18n.text("message.inherit_global_setting")}
                                        </option>
                                        <For
                                            each={move || {
                                                providers(&board.settings.get()).unwrap_or_default()
                                            }}
                                            key={|provider| field(provider, "id")}
                                            children={move |provider| {
                                                view! {
                                                    <option value={field(
                                                        &provider,
                                                        "id",
                                                    )}>{field(&provider, "name")}</option>
                                                }
                                            }}
                                        />
                                    </crate::form::Select>
                                </crate::form::Field>
                            </section>
                            <section
                                id="project-panel-launch"
                                class="form-section"
                                role="tabpanel"
                                aria-labelledby="project-tab-launch"
                                hidden={move || tab.get() != 2}
                            >
                                <ProjectServices
                                    draft={draft}
                                    pending={pending}
                                    token={token}
                                    access_dirty={[front_access_dirty, back_access_dirty]}
                                />
                            </section>
                            <section
                                id="project-panel-automation"
                                class="form-section"
                                role="tabpanel"
                                aria-labelledby="project-tab-automation"
                                hidden={move || tab.get() != 3}
                            >
                                <crate::form::Field id="project-test-command" label=Signal::derive(move || i18n.text("message.verification_command"))>
                                <crate::form::Textarea
                                    {..}
                                    id="project-test-command"
                                    maxlength="2000"
                                    rows="2"
                                    spellcheck="false"
                                    prop:value={move || draft.get().verification}
                                    on:input={move |event| {
                                        draft.update(|draft| draft.verification = event_target_value(&event))
                                    }}
                                /></crate::form::Field>
                                <p class="field-help">
                                    {move || {
                                        i18n.text(
                                            "message.this_command_runs_explicitly_in_the_task_worktree_with_your_acco",
                                        )
                                    }}
                                </p>
                                <crate::form::Field
                                    id="project-pr-target"
                                    label={Signal::derive(move || {
                                        i18n.text("message.workflow_target_branch")
                                    })}
                                >
                                    <crate::form::Input
                                        {..}
                                        id="project-pr-target"
                                        maxlength="180"
                                        spellcheck="false"
                                        prop:value={move || draft.get().target}
                                        on:input={move |event| {
                                            draft.update(|draft| draft.target = event_target_value(&event))
                                        }}
                                    />
                                </crate::form::Field>
                                <label>
                                    <crate::form::Switch
                                        {..}
                                        id="project-automation"
                                        prop:checked={move || draft.get().automation}
                                        on:change={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.automation = event_target_checked(&event);
                                                })
                                        }}
                                    />
                                    {move || i18n.text("message.allow_automatic_workflows_for_this_project")}
                                </label>
                                <label>
                                    <crate::form::Switch
                                        {..}
                                        id="project-auto-fix"
                                        prop:checked={move || draft.get().auto_fix}
                                        on:change={move |event| {
                                            draft
                                                .update(|draft| draft.auto_fix = event_target_checked(&event))
                                        }}
                                    />
                                    {move || i18n.text("message.correct_p0_p1_findings_up_to_two_cycles")}
                                </label>
                                <p class="field-help">
                                    {move || {
                                        i18n.text(
                                            "message.the_workflow_starts_agents_and_checks_after_confirmation_on_the",
                                        )
                                    }}
                                </p>
                                <crate::form::Field
                                    id="project-linked"
                                    label={Signal::derive(move || {
                                        i18n.text("message.linked_project_for_testing")
                                    })}
                                    help={Signal::derive(move || {
                                        i18n.text(
                                            "message.commands_and_the_linked_project_are_saved_for_future_launches_th",
                                        )
                                    })}
                                >
                                    <crate::form::Combobox
                                        id="project-linked"
                                        value={Signal::derive(move || draft.get().linked)}
                                        disabled={Signal::derive(move || pending.get())}
                                        describedby="project-linked-field-help"
                                        on_change={Callback::new(move |value| {
                                            draft.update(|draft| draft.linked = value)
                                        })}
                                        options={Signal::derive(move || {
                                            let mut choices = vec![
                                                crate::form::Choice {
                                                    value: String::new(),
                                                    label: i18n.text("message.no_linked_project"),
                                                },
                                            ];
                                            choices
                                                .extend(
                                                    projects
                                                        .get()
                                                        .into_iter()
                                                        .filter(|project| field(project, "id") != draft.get().id)
                                                        .map(|project| crate::form::Choice {
                                                            value: field(&project, "id"),
                                                            label: field(&project, "name"),
                                                        }),
                                                );
                                            choices
                                        })}
                                    />
                                </crate::form::Field>
                            </section>
                        </fieldset>
                        <p id="project-error" class="error" role="alert">
                            {error_text}
                        </p>
                        <crate::modal::ModalActions {..} >
                            <crate::button::Button
                                busy={Signal::derive(move || pending.get())}
                                variant={crate::button::ButtonVariant::Primary}
                                disabled={Signal::derive(move || pending.get() || branch_loading.get() || !dirty.get())}
                                class="primary"
                                {..}
                                id="save-project"
                                type="submit"
                            >
                                {move || {
                                    i18n.text(if pending.get() { "message.saving" } else { "message.save" })
                                }}
                            </crate::button::Button>
                            <crate::button::Button
                                disabled={Signal::derive(move || pending.get())}
                                {..}
                                id="reset-project"
                                type="button"
                                on:click={move |_| {
                                    if dirty.get_untracked() {
                                        ask_confirmation("settings-discard");
                                    } else {
                                        close_settings();
                                    }
                                }}
                            >
                                {move || i18n.text("message.cancel")}
                            </crate::button::Button>
                        </crate::modal::ModalActions>
                    </crate::form::Form>
                    <div
                        class="editor-secondary"
                        hidden={move || draft.get().id.is_empty() || tab.get() != 0}
                    >
                        <crate::button::Button
                            variant={crate::button::ButtonVariant::Danger}
                            disabled={Signal::derive(move || pending.get() || dirty.get())}
                            class="danger"
                            {..}
                            id="remove-project"
                            type="button"
                            on:click={move |_| ask_confirmation("remove")}
                        >
                            {move || i18n.text("message.remove_this_project")}
                        </crate::button::Button>
                    </div>

                </section>
                <p
                    class="project-selection-hint hint"
                    hidden={move || editing.get() || projects.get().is_empty()}
                >
                    {move || i18n.text("message.select_project_to_configure")}
                </p>
            </div>

        </section>
        <Show when={move || creating.get()}>
            <crate::modal::Modal
                id="project-create-dialog"
                labelledby="project-create-title"
                busy={Signal::derive(move || pending.get())}
                node_ref={NodeRef::new()}
                on_close={Callback::new(move |()| {
                    if !pending.get_untracked() {
                        if dirty.get_untracked() {
                            ask_confirmation("create-discard");
                        } else {
                            close_settings();
                        }
                    }
                })}
            >
                <crate::modal::ModalHeader
                    title_id="project-create-title"
                    title={Signal::derive(move || i18n.text("message.add_a_project"))}
                    close_id="close-project-create"
                    busy={Signal::derive(move || pending.get())}
                    on_close={Callback::new(move |()| {
                        if dirty.get_untracked() {
                            ask_confirmation("create-discard");
                        } else {
                            close_settings();
                        }
                    })}
                />
                <crate::form::Form
                    busy={Signal::derive(move || pending.get())}
                    on_submit={Callback::new(move |_| save(false))}
                    {..}
                    id="project-form"
                    class="form-section"
                >
                    <div class="section-heading">
                        <div>
                            <h3>{move || i18n.text("message.add_a_project")}</h3>
                            <p class="hint">
                                {move || i18n.text("message.a_name_and_a_folder_to_find_your_tasks")}
                            </p>
                        </div>
                    </div>
                    <fieldset disabled={move || pending.get() || editing.get()}>
                        <crate::form::Field id="project-source" label={Signal::derive(move || i18n.text("message.clone_source"))}>
                            <crate::form::Select {..} id="project-source"
                                prop:value={move || if clone_draft.get().enabled { "clone" } else { "local" }}
                                on:change={move |event| {
                                    clone_draft.update(|value| value.enabled = event_target_value(&event) == "clone");
                                    error.set(String::new());
                                }}>
                                <option value="local">{move || i18n.text("message.clone_existing_folder")}</option>
                                <option value="clone">{move || i18n.text("message.clone_repository")}</option>
                            </crate::form::Select>
                        </crate::form::Field>
                        <crate::form::Field
                            id="project-create-name"
                            label={Signal::derive(move || i18n.text("message.project_name"))}
                        >
                            <crate::form::Input
                                {..}
                                id="project-create-name"
                                autofocus=true
                                required=true
                                maxlength="100"
                                placeholder={move || i18n.text("message.my_application")}
                                prop:value={move || draft.get().name}
                                on:input={move |event| {
                                    draft.update(|draft| draft.name = event_target_value(&event))
                                }}
                            />
                        </crate::form::Field>
                        <Show when={move || clone_draft.get().enabled}>
                            <crate::form::Field id="project-clone-url" label={Signal::derive(move || i18n.text("message.clone_url"))}>
                                <crate::form::Input {..} id="project-clone-url" required=true maxlength="2000"
                                    placeholder="https://github.com/equipe/projet.git"
                                    prop:value={move || clone_draft.get().url}
                                    on:input={move |event| {
                                        let url = event_target_value(&event);
                                        error.set(String::new());
                                        let previous = atelier_core::application::project_clone::suggested_directory(&clone_draft.get_untracked().url);
                                        clone_draft.update(|value| {
                                            if value.directory.is_empty() || value.directory == previous {
                                                value.directory = atelier_core::application::project_clone::suggested_directory(&url);
                                            }
                                            if url.starts_with("ssh://") || url.contains('@') { value.connection.clear(); }
                                            value.url = url;
                                        });
                                        draft.update(|value| {
                                            if value.name.trim().is_empty() || value.name == previous { value.name = clone_draft.get_untracked().directory; }
                                        });
                                    }}/>
                            </crate::form::Field>
                            <Show when={move || !clone_draft.get().url.starts_with("ssh://") && !clone_draft.get().url.contains('@')}>
                            <crate::form::Field id="project-clone-connection" label={Signal::derive(move || i18n.text("message.clone_connection"))}
                                help={Signal::derive(move || i18n.text("message.clone_https_help"))}>
                                <crate::form::Select {..} id="project-clone-connection" prop:value={move || clone_draft.get().connection}
                                    on:change={move |event| clone_draft.update(|value| value.connection = event_target_value(&event))}>
                                    <option value="">{move || i18n.text("message.clone_public")}</option>
                                    <option value="github">"GitHub"</option>
                                    <option value="gitlab">"GitLab"</option>
                                    <option value="azure">"Azure DevOps"</option>
                                </crate::form::Select>
                            </crate::form::Field>
                            </Show>
                            <p class="hint" hidden={move || !clone_draft.get().url.starts_with("ssh://") && !clone_draft.get().url.contains('@')}>
                                {move || i18n.text("message.clone_ssh_help")}
                            </p>
                            <crate::form::Field id="project-clone-parent" label={Signal::derive(move || i18n.text("message.clone_parent"))}>
                                <div class="directory-picker">
                                    <crate::form::Input {..} id="project-clone-parent" required=true maxlength="1800"
                                        prop:value={move || clone_draft.get().parent}
                                        on:input={move |event| clone_draft.update(|value| value.parent = event_target_value(&event))}/>
                                    <crate::button::Button {..} id="browse-clone-parent" type="button" on:click={move |_| browse()}>
                                        {move || i18n.text("message.browse")}
                                    </crate::button::Button>
                                </div>
                            </crate::form::Field>
                            <crate::form::Field id="project-clone-directory" label={Signal::derive(move || i18n.text("message.clone_directory"))}
                                help={Signal::derive(move || i18n.text("message.clone_destination_help"))}>
                                <crate::form::Input {..} id="project-clone-directory" required=true maxlength="100"
                                    prop:value={move || clone_draft.get().directory}
                                    on:input={move |event| clone_draft.update(|value| value.directory = event_target_value(&event))}/>
                            </crate::form::Field>
                        </Show>
                        <Show when={move || !clone_draft.get().enabled}>
                        <crate::form::Field
                            id="project-directory"
                            label={Signal::derive(move || i18n.text("message.local_directory"))}
                        >
                            <div class="directory-picker">
                                <crate::form::Input
                                    copy=true
                                    {..}
                                    id="project-directory"
                                    required=true
                                    maxlength="2000"
                                    prop:value={move || draft.get().repo}
                                    on:input={move |event| {
                                        draft.update(|draft| draft.repo = event_target_value(&event))
                                    }}
                                />
                                <crate::button::Button
                                    {..}
                                    id="browse-project"
                                    type="button"
                                    on:click={move |_| browse()}
                                >
                                    {move || i18n.text("message.browse")}
                                </crate::button::Button>
                            </div>
                        </crate::form::Field>
                        <crate::repository_access::RepositoryAccess
                            token
                            pending
                            dirty={create_access_dirty}
                            path={Signal::derive(move || {
                                if editing.get() { String::new() } else { draft.get().repo }
                            })}
                            id="project-create-access"
                        />
                        <p class="hint">
                            {move || {
                                i18n.text(
                                    "message.choose_an_existing_folder_or_paste_its_path_worktrees_require_a",
                                )
                            }}
                        </p>
                        </Show>
                        <p class="hint" role="status" hidden={move || !cloned.get()}>{move || i18n.text("message.clone_preserved")}</p>
                        <crate::modal::ModalActions {..} >
                            <crate::button::Button
                                busy={Signal::derive(move || pending.get())}
                                variant={crate::button::ButtonVariant::Primary}
                                disabled={Signal::derive(move || !dirty.get())}
                                class="primary"
                                {..}
                                type="submit"
                            >
                                {move || i18n.text(if clone_draft.get().enabled { "message.clone_and_add" } else { "message.add_project" })}
                            </crate::button::Button>
                        </crate::modal::ModalActions>
                    </fieldset>
                    <Show when={move || !clone_operation.get().is_empty()}>
                        <p role="status">{move || i18n.text("message.clone_running")}</p>
                        <crate::button::Button {..} id="cancel-project-clone" type="button"
                            on:click={move |_| {
                                let operation = clone_operation.get_untracked();
                                if let Ok(credential) = token.get_value() {
                                    spawn_local(async move {
                                        if let Err(message) = write(&credential, Method::POST, &format!("/api/repositories/clone/{operation}/cancel"), &serde_json::json!({})).await {
                                            error.set(message);
                                        }
                                    });
                                }
                            }}>
                            {move || i18n.text("message.clone_cancel")}
                        </crate::button::Button>
                    </Show>
                    <p class="error" role="alert">
                        {error_text}
                    </p>
                </crate::form::Form>

            </crate::modal::Modal>
        </Show>
        <Show when={move || !confirmation.get().is_empty()}>
            <crate::modal::Modal
                id="project-confirmation-dialog"
                labelledby="project-confirmation-title"
                busy={Signal::derive(move || pending.get())}
                node_ref={NodeRef::new()}
                on_close={Callback::new(move |()| {
                    if !pending.get_untracked() {
                        dismiss_confirmation();
                    }
                })}
            >
                <crate::modal::ModalHeader
                    title_id="project-confirmation-title"
                    title={Signal::derive(move || {
                        i18n.text(
                            if confirmation.get() == "remove" {
                                "message.project_remove_title"
                            } else {
                                "message.project_discard_title"
                            },
                        )
                    })}
                    busy={Signal::derive(move || pending.get())}
                    on_close={Callback::new(move |()| dismiss_confirmation())}
                />
                <crate::modal::ModalBody class="project-confirmation-body">
                    <p>
                        {move || {
                            i18n.text(
                                if confirmation.get() == "remove" {
                                    "message.remove_this_project_from_the_list_repository_files_and_tasks_wil"
                                } else {
                                    "message.project_discard_description"
                                },
                            )
                        }}
                    </p>
                    <Show when={move || confirmation.get() == "remove"}>
                        <label class="check-filter">
                            <crate::form::Checkbox
                                {..}
                                id="archive-project-tasks"
                                disabled={move || pending.get()}
                                prop:checked={move || archive.get()}
                                on:change={move |event| archive.set(event_target_checked(&event))}
                            />
                            {move || i18n.text("message.also_archive_this_project_s_tasks")}
                        </label>
                        <crate::form::Field id="project-confirmation-name" label=Signal::derive(move || {
                                i18n.text("message.project_type_name_to_remove")
                                    .replace("{0}", &draft.get().name)
                            })>
                        <crate::form::Input
                            {..}
                            id="project-confirmation-name"
                            maxlength="100"
                            autocomplete="off"
                            spellcheck="false"
                            autofocus=true
                            disabled={move || pending.get()}
                            prop:value={move || confirmation_name.get()}
                            on:input={move |event| confirmation_name.set(event_target_value(&event))}
                        /></crate::form::Field>
                        <p class="error" role="alert">
                            {error_text}
                        </p>
                    </Show>
                    <crate::modal::ModalActions>
                        <crate::button::Button
                            disabled={Signal::derive(move || pending.get())}
                            {..}
                            id="cancel-project-confirmation"
                            type="button"
                            autofocus={move || confirmation.get() != "remove"}
                            on:click={move |_| dismiss_confirmation()}
                        >
                            {move || {
                                i18n.text(
                                    if confirmation.get() == "remove" {
                                        "message.cancel"
                                    } else {
                                        "message.keep_editing"
                                    },
                                )
                            }}
                        </crate::button::Button>
                        <crate::button::Button
                            variant={crate::button::ButtonVariant::Danger}
                            disabled={Signal::derive(move || {
                                pending.get()
                                    || (confirmation.get() == "remove"
                                        && (branch_loading.get()
                                            || confirmation_name.get() != draft.get().name))
                            })}
                            class="danger"
                            {..}
                            id={move || {
                                if confirmation.get() == "discard" {
                                    "discard-project-navigation"
                                } else {
                                    "confirm-project"
                                }
                            }}
                            type="button"
                            on:click={move |_| match confirmation.get_untracked() {
                                "remove" => {
                                    if confirmation_name.get_untracked() == draft.get_untracked().name {
                                        save(true);
                                    }
                                }
                                "discard" => close(),
                                _ => close_settings(),
                            }}
                        >
                            {move || {
                                i18n.text(
                                    if confirmation.get() == "remove" {
                                        "message.remove_project"
                                    } else {
                                        "message.project_discard_changes"
                                    },
                                )
                            }}
                        </crate::button::Button>
                    </crate::modal::ModalActions>
                </crate::modal::ModalBody>
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
            {[(0usize, "front", "Front"), (1usize, "back", "Back")]
                .into_iter()
                .map(move |(index, role, label)| {
                    let candidates = RwSignal::new(Vec::<Document>::new());
                    let warnings = RwSignal::new(Vec::<String>::new());
                    let error = RwSignal::new(String::new());
                    let scanned = RwSignal::new(false);
                    let identity = Memo::new(move |_| {
                        let value = draft.get();
                        (value.id, value.repo, value.services[index].repo.clone())
                    });
                    Effect::new(move |_| {
                        let _ = identity.get();
                        candidates.set(Vec::new());
                        warnings.set(Vec::new());
                        error.set(String::new());
                        scanned.set(false);
                    });
                    let detect = move |_| {
                        if pending.get_untracked() {
                            return;
                        }
                        let value = draft.get_untracked();
                        let credential = match token.get_value() {
                            Ok(value) => value,
                            Err(message) => {
                                error.set(message);
                                return;
                            }
                        };
                        pending.set(true);
                        candidates.set(Vec::new());
                        warnings.set(Vec::new());
                        error.set(String::new());
                        scanned.set(false);
                        spawn_local(async move {
                            let result = async {
                                let mut body = serde_json::json!({"role":role,"repo":value.repo});
                                body[format!("{role}Repo")] = serde_json::json!(value.services[index].repo);
                                let response = write(
                                        &credential,
                                        Method::POST,
                                        &format!("/api/projects/{}/detect-launch", value.id),
                                        &body,
                                    )
                                    .await?;
                                let result: Document = response
                                    .json()
                                    .await
                                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                                Ok::<
                                    _,
                                    String,
                                >((
                                    result
                                        .get::<Vec<Document>>("candidates")
                                        .map_err(|_| "message.invalid_server_response")?
                                        .unwrap_or_default(),
                                    result
                                        .get::<Vec<String>>("warnings")
                                        .map_err(|_| "message.invalid_server_response")?
                                        .unwrap_or_default(),
                                ))
                            }
                                .await;
                            match result {
                                Ok((found, notices)) => {
                                    candidates.set(found);
                                    warnings.set(notices);
                                    scanned.set(true);
                                }
                                Err(message) => error.set(message),
                            }
                            pending.set(false);
                        });
                    };
                    view! {
                        <div class="col-12 col-md-6">
                            <fieldset
                                class="application-launch-panel"
                                aria-labelledby={format!("project-{role}-heading")}
                            >
                                <legend id={format!("project-{role}-heading")}>{label}</legend>
                                <crate::form::Field
                                    id={format!("project-{role}-repo")}
                                    label={Signal::derive(move || i18n.text("message.service_repository"))}
                                >
                                    <crate::form::Input
                                        {..}
                                        id={format!("project-{role}-repo")}
                                        maxlength="2000"
                                        spellcheck="false"
                                        placeholder={move || i18n.text("message.main_repository_when_empty")}
                                        prop:value={move || draft.get().services[index].repo.clone()}
                                        on:input={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.services[index].repo = event_target_value(&event);
                                                })
                                        }}
                                    />
                                </crate::form::Field>
                                <crate::repository_access::RepositoryAccess
                                    token
                                    pending
                                    dirty={access_dirty[index]}
                                    path={Signal::derive(move || draft.get().services[index].repo.clone())}
                                    id={if index == 0 {
                                        "project-front-access"
                                    } else {
                                        "project-back-access"
                                    }}
                                />
                                <crate::form::Field
                                    id={format!("project-{role}-directory")}
                                    label={Signal::derive(move || i18n.text("message.service_subfolder"))}
                                >
                                    <crate::form::Input
                                        {..}
                                        id={format!("project-{role}-directory")}
                                        maxlength="2000"
                                        spellcheck="false"
                                        placeholder={move || i18n.text("message.worktree_root_when_empty")}
                                        prop:value={move || draft.get().services[index].directory.clone()}
                                        on:input={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.services[index].directory = event_target_value(
                                                        &event,
                                                    );
                                                })
                                        }}
                                    />
                                </crate::form::Field>
                                <crate::form::Field
                                    id={format!("project-{role}-command")}
                                    label={Signal::derive(move || i18n.text("message.launch_command"))}
                                >
                                    <crate::form::Textarea
                                        {..}
                                        id={format!("project-{role}-command")}
                                        rows="2"
                                        maxlength="2000"
                                        spellcheck="false"
                                        prop:value={move || draft.get().services[index].command.clone()}
                                        on:input={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.services[index].command = event_target_value(&event);
                                                })
                                        }}
                                    ></crate::form::Textarea>
                                </crate::form::Field>
                                <crate::form::Field
                                    id={format!("project-{role}-port")}
                                    label={Signal::derive(move || i18n.text("message.preferred_port"))}
                                >
                                    <crate::form::Input
                                        {..}
                                        id={format!("project-{role}-port")}
                                        type="number"
                                        min="0"
                                        max="65535"
                                        step="1"
                                        prop:value={move || draft.get().services[index].port.clone()}
                                        on:input={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.services[index].port = event_target_value(&event);
                                                })
                                        }}
                                    />
                                </crate::form::Field>
                                <p class="field-help">
                                    {move || {
                                        i18n.text(
                                            "message.0_or_empty_automatic_port_use_port_in_the_command_to_receive_the",
                                        )
                                    }}
                                </p>
                                <crate::button::Button
                                    {..}
                                    id={format!("detect-{role}")}
                                    type="button"
                                    on:click={detect}
                                >
                                    {move || i18n.text("message.detect_commands")}
                                </crate::button::Button>
                                <p
                                    class="field-help"
                                    hidden={move || !scanned.get() || !candidates.get().is_empty()}
                                >
                                    {move || {
                                        i18n.text(
                                            "message.no_command_detected_you_can_enter_your_own_command",
                                        )
                                    }}
                                </p>
                                <div class="project-list">
                                    <For
                                        each={move || {
                                            candidates.get().into_iter().enumerate().collect::<Vec<_>>()
                                        }}
                                        key={|(index, _)| *index}
                                        children={move |(_, candidate)| {
                                            let directory = field(&candidate, "directory");
                                            let command = field(&candidate, "command");
                                            let runnable = !command.is_empty();
                                            let text = format!(
                                                "{} · {} · {}",
                                                field(&candidate, "technology"),
                                                if directory.is_empty() { "." } else { &directory },
                                                command,
                                            );
                                            view! {
                                                <crate::button::Button
                                                    disabled={!runnable}
                                                    class="launch-candidate"
                                                    {..}
                                                    data-role={role}
                                                    type="button"
                                                    on:click={move |_| {
                                                        draft
                                                            .update(|draft| {
                                                                draft.services[index].directory = directory.clone();
                                                                draft.services[index].command = command.clone();
                                                            });
                                                    }}
                                                >
                                                    {text}
                                                </crate::button::Button>
                                            }
                                        }}
                                    />
                                </div>
                                <div role="status">
                                    {move || {
                                        warnings
                                            .get()
                                            .into_iter()
                                            .map(|warning| view! { <p>{i18n.text(&warning)}</p> })
                                            .collect_view()
                                    }}
                                </div>
                                <p class="error" role="alert">
                                    {move || i18n.text(&error.get())}
                                </p>
                            </fieldset>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
