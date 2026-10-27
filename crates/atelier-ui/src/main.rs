use crate::i18n::I18n;
use atelier_core::contracts::{Board, Card};
use gloo_net::http::{Method, Request, RequestBuilder, Response};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

mod agent_cli;
mod agent_diagnostics;
mod agent_profiles;
mod agent_usage;
mod automation;
mod backups;
mod brainstorm;
mod brainstorm_page;
mod commits;
mod connector_effects;
mod connector_ticket;
mod connectors;
mod files;
mod forge;
mod forge_checks;
mod forge_content;
mod forge_publication;
mod forge_task;
mod git_push;
mod i18n;
mod jira;
mod jira_batch;
mod jira_creation;
mod jira_refresh;
mod jira_task;
mod live;
mod preparation;
mod projects;
mod queue;
mod repository_access;
mod review;
mod runs;
mod services;
mod settings;
mod task_editor;
mod tempo;
mod tempo_export;
mod time;
mod time_report;
mod updates;
mod workspace_views;
mod worktree;
use agent_profiles::AgentProfiles;
use backups::Backups;
use brainstorm_page::BrainstormPage;
use live::BoardState;
use projects::Projects;
use settings::Settings;
use task_editor::TaskEditor;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceView {
    Overview,
    Board,
    Brainstorm,
    Agents,
    Processes,
    Worktrees,
    Time,
    Archives,
    Settings,
    Diagnostics,
    Updates,
    Task,
}

impl WorkspaceView {
    fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Board => "board",
            Self::Brainstorm => "brainstorm",
            Self::Agents => "agents",
            Self::Processes => "processes",
            Self::Worktrees => "worktrees",
            Self::Time => "time",
            Self::Archives => "archives",
            Self::Settings => "settings",
            Self::Diagnostics => "diagnostics",
            Self::Updates => "updates",
            Self::Task => "task",
        }
    }
    fn from_key(key: &str) -> Self {
        match key {
            "board" => Self::Board,
            "brainstorm" => Self::Brainstorm,
            "agents" => Self::Agents,
            "processes" => Self::Processes,
            "worktrees" => Self::Worktrees,
            "time" => Self::Time,
            "archives" => Self::Archives,
            "settings" => Self::Settings,
            "updates" => Self::Updates,
            _ => Self::Overview,
        }
    }
    fn heading(self) -> (&'static str, &'static str) {
        match self {
            Self::Settings | Self::Updates => {
                ("Paramètres", "Réglages généraux, modules et connexions.")
            }
            Self::Brainstorm => (
                "Brainstorming",
                "Explorez une idée. Choisissez vos prochaines tâches. Jira reste facultatif.",
            ),
            Self::Overview => (
                "Vue d’ensemble",
                "Vos prochaines actions, en un coup d’œil.",
            ),
            Self::Board => (
                "Tableau des tâches",
                "Préparez, lancez, vérifiez. Une étape à la fois.",
            ),
            Self::Agents => (
                "Activité des agents",
                "Suivez les exécutions et retrouvez leurs résultats.",
            ),
            Self::Processes => (
                "Processus",
                "Lancez et pilotez les fronts et les backs de vos projets.",
            ),
            Self::Worktrees => (
                "Branches & pull requests",
                "Retrouvez les dossiers de travail, les branches et les PR.",
            ),
            Self::Time => (
                "Temps passé",
                "Relisez vos saisies avant leur envoi au service connecté.",
            ),
            Self::Archives => (
                "Archives",
                "Retrouvez l’historique et restaurez une tâche quand vous en avez besoin.",
            ),
            Self::Diagnostics => ("Diagnostic", "Tester les connexions"),
            Self::Task => ("", ""),
        }
    }
}

#[derive(Clone, Copy)]
pub struct NavigationGuard(
    pub RwSignal<Option<Callback<Callback<()>>>>,
    pub RwSignal<u64>,
);

fn focus_element(id: &'static str) {
    request_animation_frame(move || {
        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(id))
            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = element.focus();
        }
    });
}

const STATUSES: [(&str, &str); 5] = [
    ("todo", "À faire"),
    ("doing", "En cours"),
    ("review", "À vérifier"),
    ("done", "Terminé"),
    ("standby", "En attente"),
];

async fn write(
    token: &str,
    method: Method,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response, String> {
    let response = RequestBuilder::new(path)
        .method(method)
        .header("Authorization", &format!("Bearer {token}"))
        .header("X-Local-Request", "1")
        .json(body)
        .map_err(|_| "Requête invalide.")?
        .send()
        .await
        .map_err(|_| "Connexion interrompue. Vérifie l’état avant de réessayer.")?;
    checked_response(response).await
}

async fn checked_response(response: Response) -> Result<Response, String> {
    if !response.ok() {
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "Réponse du serveur invalide.")?;
        return Err(body
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("Opération refusée.")
            .to_owned());
    }
    Ok(response)
}

fn replace_card(cards: RwSignal<Vec<Card>>, card: Card) {
    cards.update(|cards| {
        if let Some(existing) = cards.iter_mut().find(|existing| existing.id == card.id) {
            *existing = card;
        } else {
            cards.push(card);
        }
    });
}

fn requested_task() -> Option<String> {
    let path = web_sys::window()?.location().pathname().ok()?;
    let id = path.strip_prefix("/tasks/")?;
    if id.is_empty() || id.contains('/') {
        return None;
    }
    js_sys::decode_uri_component(id).ok().map(String::from)
}

fn requested_parameter(key: &str) -> Option<String> {
    let location = web_sys::window()?.location();
    if key == "view" {
        return Some(location.pathname().ok()?.trim_matches('/').to_owned());
    }
    web_sys::UrlSearchParams::new_with_str(&location.search().ok()?)
        .ok()?
        .get(key)
}

fn current_path() -> String {
    web_sys::window()
        .map(|window| {
            format!(
                "{}{}",
                window.location().pathname().unwrap_or_default(),
                window.location().search().unwrap_or_default()
            )
        })
        .unwrap_or_else(|| "/overview".into())
}
fn session() -> Result<String, String> {
    let window = web_sys::window().ok_or("Navigateur indisponible.")?;
    let location = window.location();
    let params = web_sys::UrlSearchParams::new_with_str(
        location
            .hash()
            .map_err(|_| "Adresse invalide.")?
            .trim_start_matches('#'),
    )
    .map_err(|_| "Adresse invalide.")?;
    let local = window
        .local_storage()
        .map_err(|_| "Stockage local indisponible.")?;
    let tab = window
        .session_storage()
        .map_err(|_| "Stockage de session indisponible.")?;
    let token = params
        .get("session")
        .or_else(|| local.as_ref()?.get_item("atelier-session").ok()?)
        .or_else(|| tab.as_ref()?.get_item("atelier-session").ok()?)
        .ok_or("Session absente. Rouvre Atelier avec son lanceur.")?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Session invalide.".into());
    }
    let address = format!(
        "{}{}",
        location.pathname().map_err(|_| "Adresse invalide.")?,
        location.search().map_err(|_| "Adresse invalide.")?,
    );
    window
        .history()
        .and_then(|history| history.replace_state_with_url(&history.state()?, "", Some(&address)))
        .map_err(|_| "Navigation indisponible.")?;
    for storage in [local, tab].into_iter().flatten() {
        let _ = storage.set_item("atelier-session", &token);
    }
    Ok(token)
}

async fn read(token: &str, path: &str) -> Result<Response, String> {
    let response = Request::get(path)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|_| "Connexion interrompue.")?;
    checked_response(response).await
}

async fn load(token: &str) -> Result<Board, String> {
    read(token, "/api/state")
        .await?
        .json::<Board>()
        .await
        .map_err(|_| "Réponse du serveur invalide.".into())
}

#[component]
fn App() -> impl IntoView {
    let i18n = I18n::default();
    provide_context(i18n);
    let board = BoardState::default();
    let navigation_guard = NavigationGuard(RwSignal::new(None), RwSignal::new(0));
    provide_context(navigation_guard);
    let view = RwSignal::new(if requested_task().is_some() {
        WorkspaceView::Task
    } else {
        WorkspaceView::from_key(requested_parameter("view").as_deref().unwrap_or("overview"))
    });
    let return_view = RwSignal::new(WorkspaceView::Board);
    let show_agents = RwSignal::new(false);
    let show_backups = RwSignal::new(false);
    let show_diagnostics = RwSignal::new(false);
    let deleting_card = RwSignal::new(None::<Card>);
    let dragging = RwSignal::new(None::<(String, String)>);
    let command_open = RwSignal::new(false);
    let settings_section = RwSignal::new(
        if requested_parameter("section").as_deref() == Some("jira") {
            "jira"
        } else {
            "general"
        },
    );
    let cards = board.cards;
    let projects = board.projects;
    let templates = board.templates;
    let template = RwSignal::new(String::new());
    let remove_template = RwSignal::new(false);
    let template_pending = RwSignal::new(false);
    let search = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let focus_filter = RwSignal::new("all".to_owned());
    let loading = RwSignal::new(true);
    let title = RwSignal::new(String::new());
    let title_input = NodeRef::<leptos::html::Input>::new();
    let new_dialog = NodeRef::<leptos::html::Dialog>::new();
    let description = RwSignal::new(String::new());
    let instructions = RwSignal::new(String::new());
    let new_repo = RwSignal::new(String::new());
    let new_agent = RwSignal::new(String::new());
    let new_initialized = RwSignal::new(false);
    let new_mode = RwSignal::new("edit".to_owned());
    let new_model = RwSignal::new(String::new());
    let new_effort = RwSignal::new(String::new());
    let create_error = RwSignal::new(String::new());
    let template_candidate = RwSignal::new(None::<String>);
    let apply_template = Callback::new(move |id: String| {
        if let Some(item) = templates
            .with_untracked(|items| items.iter().find(|item| field(item, "id") == id).cloned())
        {
            description.set(field(&item, "description"));
            instructions.set(field(&item, "instructions"));
            new_mode.set(field(&item, "mode"));
            template.set(id);
        }
        template_candidate.set(None);
    });
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let selected = RwSignal::new(None::<Card>);
    let show_projects = RwSignal::new(false);
    let token = StoredValue::new(session());
    let committed_path = RwSignal::new(current_path());
    let history_index = RwSignal::new(
        web_sys::window()
            .and_then(|window| window.history().ok())
            .and_then(|history| history.state().ok())
            .and_then(|state| state.as_f64())
            .unwrap_or(0.0) as i32,
    );
    if let Some(history) = web_sys::window().and_then(|window| window.history().ok()) {
        let _ = history.replace_state_with_url(
            &wasm_bindgen::JsValue::from_f64(history_index.get_untracked() as f64),
            "",
            None,
        );
    }
    let write_address = Callback::new(move |path: String| {
        if path == committed_path.get_untracked() {
            return;
        }
        let next = history_index.get_untracked() + 1;
        match web_sys::window()
            .ok_or("Navigateur indisponible.".to_owned())
            .and_then(|window| {
                window
                    .history()
                    .map_err(|_| "Navigation indisponible.".to_owned())
            })
            .and_then(|history| {
                history
                    .push_state_with_url(
                        &wasm_bindgen::JsValue::from_f64(next as f64),
                        "",
                        Some(&path),
                    )
                    .map_err(|_| "Navigation indisponible.".to_owned())
            }) {
            Ok(()) => {
                history_index.set(next);
                committed_path.set(path);
            }
            Err(message) => error.set(message),
        }
    });
    match token.get_value() {
        Ok(token) => {
            if let Err(message) = board.listen(token.clone()) {
                board.sync_error.set(message);
            }
            let token = token.clone();
            spawn_local(async move {
                match load(&token).await {
                    Ok(value) => {
                        board.apply(value);
                        let requested = requested_task();
                        let active = requested.as_ref().and_then(|id| {
                            cards.with_untracked(|cards| {
                                cards.iter().find(|card| &card.id == id).cloned()
                            })
                        });
                        if requested.is_some() && active.is_none() {
                            error.set(
                                "Cette tâche est introuvable. Le tableau reste disponible.".into(),
                            );
                            view.set(WorkspaceView::Board);
                        }
                        selected.set(active);
                    }
                    Err(message) => error.set(message),
                }
                loading.set(false);
            });
        }
        Err(message) => {
            error.set(message.clone());
            loading.set(false);
        }
    }
    let visible = move |brainstorms: bool| {
        let query = search.get().trim().to_lowercase();
        let repo = repository.get();
        cards
            .get()
            .into_iter()
            .filter(|card| {
                card.fields.truthy("brainstormSession").unwrap_or(false) == brainstorms
                    && card.fields.truthy("archived").unwrap_or(false)
                        == (view.get() == WorkspaceView::Archives)
                    && (repo.is_empty() || field(&card.fields, "repo") == repo)
                    && (query.is_empty()
                        || format!(
                            "{} {} {}",
                            String::from_utf16_lossy(&card.title.0),
                            field(&card.fields, "description"),
                            card.fields
                                .get::<atelier_core::document::Document>("ticket")
                                .ok()
                                .flatten()
                                .map(|ticket| field(&ticket, "key"))
                                .unwrap_or_default()
                        )
                        .to_lowercase()
                        .contains(&query))
            })
            .collect::<Vec<_>>()
    };
    let commit_view = Callback::new(move |next: WorkspaceView| {
        selected.set(None);
        view.set(next);
        let path =
            if next == WorkspaceView::Settings && settings_section.get_untracked() != "general" {
                format!("/settings?section={}", settings_section.get_untracked())
            } else {
                format!("/{}", next.key())
            };
        write_address.run(path);
        focus_element("main-content");
    });
    let navigate = Callback::new(move |next: WorkspaceView| {
        let continuation = Callback::new(move |()| commit_view.run(next));
        if let Some(guard) = navigation_guard.0.get_untracked() {
            guard.run(continuation);
        } else {
            continuation.run(());
        }
    });
    let open_task = Callback::new(move |id: String| {
        let continuation = Callback::new(move |()| {
            error.set(String::new());
            write_address.run(format!(
                "/tasks/{}",
                String::from(js_sys::encode_uri_component(&id))
            ));
            let card = cards.with(|cards| cards.iter().find(|card| card.id == id).cloned());
            if card.is_none() {
                error.set("Cette tâche est introuvable. Le tableau reste disponible.".into());
                return;
            }
            if view.get_untracked() != WorkspaceView::Task {
                return_view.set(view.get_untracked());
            }
            selected.set(card);
            view.set(WorkspaceView::Task);
            focus_element("task-editor");
        });
        if let Some(guard) = navigation_guard.0.get_untracked() {
            guard.run(continuation);
        } else {
            continuation.run(());
        }
    });
    let open_new = Callback::new(move |()| {
        if loading.get_untracked() || pending.get_untracked() {
            return;
        }
        let continuation = Callback::new(move |()| {
            if matches!(
                view.get_untracked(),
                WorkspaceView::Task
                    | WorkspaceView::Settings
                    | WorkspaceView::Updates
                    | WorkspaceView::Brainstorm
            ) {
                commit_view.run(WorkspaceView::Board);
            }
            if !new_initialized.get_untracked() {
                let selected_repo = repository.get_untracked();
                let repo = if !selected_repo.is_empty() {
                    selected_repo
                } else {
                    projects.with_untracked(|items| {
                        if items.len() == 1 {
                            field(&items[0], "repo")
                        } else {
                            board
                                .settings
                                .with_untracked(|settings| field(settings, "repo"))
                        }
                    })
                };
                let agent = projects.with_untracked(|items| {
                    items
                        .iter()
                        .find(|project| field(project, "repo") == repo)
                        .map(|project| field(project, "agent"))
                        .filter(|agent| !agent.is_empty())
                        .unwrap_or_else(|| {
                            board
                                .settings
                                .with_untracked(|settings| field(settings, "agent"))
                        })
                });
                new_repo.set(repo);
                new_agent.set(agent);
                new_initialized.set(true);
            }
            if let Some(dialog) = new_dialog.get_untracked() {
                let _ = dialog.show_modal();
            }
            focus_element("task-title");
        });
        if let Some(guard) = navigation_guard.0.get_untracked() {
            guard.run(continuation);
        } else {
            continuation.run(());
        }
    });
    let card_action = Callback::new(move |(id, action): (String, &'static str)| {
        if pending.get_untracked() {
            return;
        }
        let Some(card) =
            cards.with_untracked(|cards| cards.iter().find(|card| card.id == id).cloned())
        else {
            return;
        };
        error.set(String::new());
        if action == "delete" {
            deleting_card.set(Some(card));
            return;
        }
        let (method, path, body) = if action == "stop" {
            let Some(run) = board.runs.with_untracked(|runs| {
                runs.iter()
                    .find(|run| {
                        field(run, "cardId") == id
                            && matches!(
                                field(run, "status").as_str(),
                                "starting" | "running" | "stopping"
                            )
                    })
                    .cloned()
            }) else {
                return;
            };
            (
                Method::POST,
                format!("/api/runs/{}/cancel", field(&run, "id")),
                serde_json::json!({}),
            )
        } else if action == "confirm-delete" {
            (
                Method::DELETE,
                format!("/api/cards/{id}"),
                serde_json::json!({"confirmed":true}),
            )
        } else {
            (
                Method::POST,
                format!("/api/cards/{id}/archive"),
                serde_json::json!({"archived":true}),
            )
        };
        let Ok(token) = token.get_value() else {
            return;
        };
        pending.set(true);
        spawn_local(async move {
            match write(&token, method, &path, &body).await {
                Ok(_) => {
                    deleting_card.set(None);
                    if let Err(message) = board.refresh(&token).await {
                        error.set(message);
                    }
                }
                Err(message) => {
                    error.set(message);
                    let _ = board.refresh(&token).await;
                }
            }
            pending.set(false);
        });
    });
    let move_card = Callback::new(move |status: &'static str| {
        let Some((id, expected)) = dragging.get_untracked() else {
            return;
        };
        dragging.set(None);
        if pending.get_untracked() || expected == status {
            return;
        }
        let Ok(token) = token.get_value() else {
            return;
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            match write(
                &token,
                Method::PATCH,
                &format!("/api/cards/{id}"),
                &serde_json::json!({"status":status,"expected":{"status":expected}}),
            )
            .await
            {
                Ok(response) => match response.json::<Card>().await {
                    Ok(card) => replace_card(cards, card),
                    Err(_) => error.set("Réponse du serveur invalide.".into()),
                },
                Err(message) => error.set(message),
            }
            let _ = board.refresh(&token).await;
            pending.set(false);
        });
    });
    let close_new = move || {
        if !pending.get_untracked() && !template_pending.get_untracked() {
            if let Some(dialog) = new_dialog.get_untracked() {
                dialog.close();
            }
            focus_element("create-button");
        }
    };
    let workspace_controls = move || {
        !matches!(
            view.get(),
            WorkspaceView::Settings
                | WorkspaceView::Updates
                | WorkspaceView::Task
                | WorkspaceView::Brainstorm
        )
    };
    let returning_history = StoredValue::new(None::<(String, i32)>);
    let approved_history = StoredValue::new(false);
    let history_listener = window_event_listener(leptos::ev::popstate, move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(history) = window.history() else {
            return;
        };
        let target_index = history
            .state()
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0) as i32;
        if approved_history.get_value() {
            approved_history.set_value(false);
            history_index.set(target_index);
            committed_path.set(current_path());
            if let Some(id) = requested_task() {
                let card =
                    cards.with_untracked(|cards| cards.iter().find(|card| card.id == id).cloned());
                if card.is_some() {
                    if view.get_untracked() != WorkspaceView::Task {
                        return_view.set(view.get_untracked());
                    }
                    selected.set(card);
                    view.set(WorkspaceView::Task);
                    focus_element("editor-heading");
                } else {
                    selected.set(None);
                    view.set(WorkspaceView::Board);
                    error.set("Cette tâche est introuvable. Le tableau reste disponible.".into());
                }
            } else {
                selected.set(None);
                settings_section.set(
                    if requested_parameter("section").as_deref() == Some("jira") {
                        "jira"
                    } else {
                        "general"
                    },
                );
                view.set(WorkspaceView::from_key(
                    requested_parameter("view").as_deref().unwrap_or("overview"),
                ));
                focus_element("main-content");
            }
            return;
        }
        if let Some((_, destination_index)) = returning_history.get_value() {
            if target_index != history_index.get_untracked() {
                return;
            }
            returning_history.set_value(None);
            if window
                .document()
                .and_then(|document| document.query_selector("dialog[open]").ok().flatten())
                .is_some()
            {
                return;
            }
            let delta = destination_index - target_index;
            let continuation = Callback::new(move |()| {
                approved_history.set_value(true);
                if let Some(history) = web_sys::window().and_then(|window| window.history().ok())
                    && history.go_with_delta(delta).is_err()
                {
                    approved_history.set_value(false);
                    error.set("Navigation indisponible.".into());
                }
            });
            if let Some(guard) = navigation_guard.0.get_untracked() {
                guard.run(continuation);
            } else {
                continuation.run(());
            }
        } else if target_index != history_index.get_untracked() {
            // Return to the current entry before asking a form to discard its draft.
            // Cancellation therefore preserves both its mounted state and the history position.
            returning_history.set_value(Some((current_path(), target_index)));
            if history
                .go_with_delta(history_index.get_untracked() - target_index)
                .is_err()
            {
                returning_history.set_value(None);
                error.set("Navigation indisponible.".into());
            }
        }
    });
    on_cleanup(move || history_listener.remove());
    let shortcuts = window_event_listener(leptos::ev::keydown, move |event| {
        let editing = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|element| {
                element
                    .closest("input,textarea,select,[contenteditable=true]")
                    .ok()
                    .flatten()
                    .is_some()
            });
        let modal = web_sys::window()
            .and_then(|window| window.document())
            .is_some_and(|document| {
                document
                    .query_selector("dialog[open]")
                    .ok()
                    .flatten()
                    .is_some()
            });
        if event.repeat() || event.is_composing() || event.alt_key() || editing || modal {
            return;
        }
        let key = event.key().to_lowercase();
        if (event.ctrl_key() || event.meta_key()) && key == "k" {
            event.prevent_default();
            command_open.set(true);
            return;
        }
        if event.ctrl_key() || event.meta_key() || event.shift_key() {
            return;
        }
        if key == "n" {
            event.prevent_default();
            open_new.run(());
        } else if key == "/" {
            event.prevent_default();
            let continuation = Callback::new(move |()| {
                commit_view.run(WorkspaceView::Board);
                focus_element("board-search");
            });
            if let Some(guard) = navigation_guard.0.get_untracked() {
                guard.run(continuation);
            } else {
                continuation.run(());
            }
        }
    });
    on_cleanup(move || shortcuts.remove());
    let draft_unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if pending.get_untracked()
            || !title.get_untracked().is_empty()
            || !description.get_untracked().is_empty()
            || !instructions.get_untracked().is_empty()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || draft_unload.remove());
    view! {
        <button class="skip-link" type="button" on:click=move |_|focus_element("main-content")>{move || i18n.text("Aller au contenu")}</button>
        <aside class="sidebar" aria-label=move || i18n.text("Navigation principale")>
            <a class="brand" href="/overview" aria-label=move || i18n.text("Atelier Kanban, accueil") on:click=move |event| {event.prevent_default();navigate.run(WorkspaceView::Overview);}>
                <img class="brand-logo" src="/atelier-kanban.png" width="40" height="40" alt="" /><span>"Atelier"<span class="brand-subtitle">"Kanban"</span></span><small class="brand-version" id="app-version">{env!("CARGO_PKG_VERSION")}</small>
            </a>
            <div class="workspace-label">{move || i18n.text("ESPACE PERSONNEL")}</div>
            <button class="nav-item" class:nav-active=move || view.get()==WorkspaceView::Brainstorm data-view="brainstorm" id="brainstorm-button" aria-current=move || if view.get()==WorkspaceView::Brainstorm {"page"}else{"false"} on:click=move |_|navigate.run(WorkspaceView::Brainstorm)><NavigationIcon kind="brainstorm" /><span class="nav-label">{move || i18n.text("Brainstorming")}</span></button>
            <button class="nav-item" class:nav-active=move || view.get()==WorkspaceView::Overview id="overview-button" aria-current=move || if view.get()==WorkspaceView::Overview {"page"}else{"false"} on:click=move |_|navigate.run(WorkspaceView::Overview)><NavigationIcon kind="overview" /><span class="nav-label">{move || i18n.text("Vue d’ensemble")}</span></button>
            <button class="nav-item" id="manage-projects" type="button" disabled=move || loading.get() on:click=move |_| show_projects.set(true)><NavigationIcon kind="projects" /><span class="nav-label">{move || i18n.text("Mes projets")}</span></button>
            <button class="nav-item" id="manage-settings" type="button" class:nav-active=move || matches!(view.get(),WorkspaceView::Settings|WorkspaceView::Updates) aria-current=move || if matches!(view.get(),WorkspaceView::Settings|WorkspaceView::Updates){"page"}else{"false"} disabled=move || loading.get() on:click=move |_| {settings_section.set("general");navigate.run(WorkspaceView::Settings);}><NavigationIcon kind="settings" /><span class="nav-label">{move || i18n.text("Réglages")}</span></button>
            <button class="nav-item" id="diagnostics-button" type="button" disabled=move || loading.get() on:click=move |_|show_diagnostics.set(true)><NavigationIcon kind="diagnostics" /><span class="nav-label">{move || i18n.text("Diagnostic")}</span></button>
            <button class="nav-item" id="manage-backups" type="button" disabled=move || loading.get() on:click=move |_| show_backups.set(true)><NavigationIcon kind="backups" /><span class="nav-label">{move || i18n.text("Sauvegardes")}</span></button>
            <button class="nav-item" id="check-updates" type="button" disabled=move || loading.get() on:click=move |_|navigate.run(WorkspaceView::Updates)><span aria-hidden="true">"↻"</span><span class="nav-label">{move || i18n.text("Mises à jour")}</span></button>
            <div class="sidebar-bottom"><div class="workspace-label">{move || i18n.text("VOS AGENTS")}</div><div id="agents"><For each=move || atelier_core::domain::agent_options::providers(&board.settings.get()).unwrap_or_default() key=|profile|field(profile,"id") children=move |profile|view!{<button class="agent-row" type="button" on:click=move |_|show_agents.set(true)><span class="agent-avatar">{field(&profile,"name").chars().next().unwrap_or('A')}</span><span class="agent-row-copy"><strong>{field(&profile,"name")}</strong><small><span class="online-dot offline"></span>{move ||i18n.text("Non vérifié")}</small></span><span class="agent-chevron" aria-hidden="true">"›"</span></button>} /><button id="manage-agents" class="text-button" type="button" disabled=move || loading.get() on:click=move |_|show_agents.set(true)>{move || i18n.text("Gérer les agents")}</button></div><div class="local-note"><span class="online-dot"></span>" "{move || i18n.text("Sur ce PC")}<small>{move || i18n.text("Vos cartes restent ici.")}</small></div></div>
        </aside>
        <main id="main-content" tabindex="-1">
            <header class="topbar"><span>{move || i18n.text("Mon espace")}<span class="slash">"/"</span><strong id="breadcrumb">{move || projects.with(|items|items.iter().find(|project|field(project,"repo")==repository.get()).map(|project|field(project,"name"))).unwrap_or_else(||i18n.text("Tous les projets"))}</strong></span><button class="command-trigger" id="command-button" on:click=move |_|command_open.set(true)>{move || i18n.text("Rechercher / aller à…")}<kbd>"Ctrl K"</kbd></button><span class="local-badge">"LOCAL "<span class="online-dot"></span></span></header>
            <section class="heading" hidden=move || view.get()==WorkspaceView::Task><div><div class="eyebrow">{move || i18n.text("VOTRE ESPACE DE TRAVAIL")}</div><h1 id="workspace-title">{move || i18n.text(view.get().heading().0)}</h1><p id="workspace-subtitle">{move || i18n.text(view.get().heading().1)}</p></div><div class="heading-actions"><button id="create-button" class="primary" type="button" hidden=move || !workspace_controls() disabled=move || pending.get() || loading.get() on:click=move |_|open_new.run(())>"＋ "{move || i18n.text("Nouvelle tâche")}<kbd>"N"</kbd></button></div></section>
            <section class="toolbar" hidden=move || !workspace_controls()><nav class="tabs" aria-label=move || i18n.text("Vues du projet")>{[(WorkspaceView::Overview,"Aperçu"),(WorkspaceView::Board,"Kanban"),(WorkspaceView::Agents,"Agents"),(WorkspaceView::Processes,"Processus"),(WorkspaceView::Worktrees,"Branches & PR"),(WorkspaceView::Time,"Temps"),(WorkspaceView::Archives,"Archives")].into_iter().map(move |(target,label)|view!{<button class="view-tab" class:selected-tab=move || view.get()==target data-view=target.key() id=match target{WorkspaceView::Time=>"view-time-report",WorkspaceView::Archives=>"show-archives",_=>target.key()} aria-pressed=move ||(view.get()==target).to_string() on:click=move |_|navigate.run(target)><NavigationIcon kind=target.key() view_icon=true /><span>{move ||i18n.text(label)}</span></button>}).collect_view()}</nav></section>
            <section class="toolbar-actions" aria-label=move || i18n.text("Projet et recherche") hidden=move || !workspace_controls()>
                <label class="project-choice" for="board-project">{move || i18n.text("Projet")}<select id="board-project" prop:value=move ||repository.get() on:change=move |event| {repository.set(event_target_value(&event));focus_filter.set("all".into());search.set(String::new());}><option value="">{move ||i18n.text("Tous les projets")}</option><For each=move ||projects.get() key=|project|field(project,"id") children=move |project|view!{<option value=field(&project,"repo")>{field(&project,"name")}</option>} /></select></label>
                <input id="board-search" type="search" hidden=move || view.get()!=WorkspaceView::Board placeholder=move ||i18n.text("Titre, ticket Jira ou description…") aria-label=move ||i18n.text("Rechercher une tâche") prop:value=move ||search.get() on:input=move |event|search.set(event_target_value(&event)) />
                <button id="sync-button" type="button" on:click=move |_| {settings_section.set("jira");navigate.run(WorkspaceView::Settings);}>"↻ "{move ||i18n.text("Synchroniser les tickets")}</button>
            </section>
            <div class="board-meta" hidden=move || !workspace_controls()><span id="repo-label">{move ||projects.with(|items|items.iter().find(|project|field(project,"repo")==repository.get()).map(|project|format!("{} · {}",field(project,"name"),field(project,"repo")))).unwrap_or_else(||i18n.text("Tous vos projets · choisissez un projet pour cibler le tableau"))}</span><span id="live-status" role="status" hidden=move || token.get_value().is_err() data-connected=move ||board.connected.get().to_string()>{move ||i18n.text(&(if !board.sync_error.get().is_empty(){board.sync_error.get()}else if board.connected.get(){"Tableau à jour".into()}else{"Actualisation interrompue. Reconnexion automatique…".into()}))}</span><span id="activity">{move ||{let count=board.runs.with(|runs|runs.iter().filter(|run|field(run,"status")=="running").count());if count==0{i18n.text("À votre rythme")}else{i18n.text("{0} agent(s) au travail").replace("{0}",&count.to_string())}}}</span></div>
            <div id="focus-bar" class="focus-bar" hidden=move || view.get()!=WorkspaceView::Board || cards.get().is_empty()><span class="results-count"><strong id="total">{move ||visible(false).into_iter().filter(|card|matches_focus(card,&cards.get(),&board.runs.get(),&focus_filter.get())).count()}</strong>" "{move ||i18n.text("tâche(s)")}</span><div class="focus-options">{[("all","Toutes"),("review","À vérifier"),("attention","À reprendre")].into_iter().map(move |(filter,label)|view!{<button data-focus=filter aria-pressed=move ||(focus_filter.get()==filter).to_string() on:click=move |_|focus_filter.set(filter.into())>{move ||i18n.text(label)}<span hidden=filter=="all">{move ||visible(false).iter().filter(|card|matches_focus(card,&cards.get(),&board.runs.get(),filter)).count()}</span></button>}).collect_view()}</div><span class="keyboard-hint"><kbd>"/"</kbd>" "{move ||i18n.text("rechercher")}" · "<kbd>"N"</kbd>" "{move ||i18n.text("nouvelle tâche")}</span></div>
            <crate::time::TimerBanner board token on_task=open_task />
            <p role="alert" id="error">{move || i18n.text(&error.get())}</p><p class="loading" role="status" hidden=move || !loading.get()>{move ||i18n.text("Chargement du tableau…")}</p>
            <section id="welcome" class="welcome-guide" hidden=move || !matches!(view.get(),WorkspaceView::Board|WorkspaceView::Overview) || !visible(false).is_empty() || loading.get()><div><div class="eyebrow">{move ||i18n.text("BIENVENUE DANS VOTRE ATELIER")}</div><h2>{move ||i18n.text("Un projet. Une tâche. On avance.")}</h2><p class="hint">{move ||i18n.text("Commencez localement. Ajoutez les connecteurs dont votre équipe a besoin.")}</p></div><div class="setup-steps"><button id="welcome-projects" on:click=move |_|show_projects.set(true)><span class="step-number">"1"</span><span><strong>{move ||i18n.text("Choisir un projet")}</strong><small>{move ||i18n.text("Associez un nom à votre dossier de code.")}</small></span><span>"→"</span></button><button id="welcome-create" on:click=move |_|open_new.run(())><span class="step-number">"2"</span><span><strong>{move ||i18n.text("Créer la première tâche")}</strong><small>{move ||i18n.text("Décrivez le résultat attendu et choisissez votre agent.")}</small></span><span>"→"</span></button></div><button id="welcome-settings" class="text-button" on:click=move |_|navigate.run(WorkspaceView::Settings)>{move ||i18n.text("Configurer les modules et connexions…")}</button></section>
            <section aria-label=move || i18n.text("Tableau des tâches") id="prototype-board" class="board" tabindex="0" hidden=move ||view.get()!=WorkspaceView::Board || visible(false).is_empty() aria-busy=move || loading.get().to_string()>
                <For each=|| STATUSES key=|(status, _)| *status children=move |(status, label)| {
                    let column = move || if view.get()==WorkspaceView::Archives {Vec::new()} else {visible(false).into_iter().filter(|card| field(&card.fields, "status") == status && matches_focus(card,&cards.get(),&board.runs.get(),&focus_filter.get())).collect::<Vec<_>>()};
                    view! { <section class=format!("column {status}") data-status=status aria-label=move || i18n.text(label) on:dragover=move |event:leptos::ev::DragEvent|{if dragging.get_untracked().is_some()&&!pending.get_untracked(){event.prevent_default();if let Some(transfer)=event.data_transfer(){transfer.set_drop_effect("move");}}} on:drop=move |event:leptos::ev::DragEvent|{if dragging.get_untracked().is_some(){event.prevent_default();move_card.run(status);}}><div class="column-heading"><span class="status-dot"></span>{move || i18n.text(label)}<span class="count">{move || column().len()}</span></div><For each=column key=|card| card.id.clone() children=move |card| {
                        let id = StoredValue::new(card.id);
                        let current = move || cards.with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned());
                        let running=move || board.runs.with(|runs|runs.iter().any(|run|field(run,"cardId")==id.get_value()&&matches!(field(run,"status").as_str(),"starting"|"running"|"stopping")));
                        let busy=move ||pending.get()||current().is_some_and(|card|card.fields.truthy("publishing").unwrap_or(false)||card.fields.get::<atelier_core::document::Document>("automation").ok().flatten().is_some_and(|flow|field(&flow,"state")=="running"));
                        view! { <article class="card" data-id=id.get_value() draggable=move ||(!busy()).to_string() class:dragging=move ||dragging.with(|value|value.as_ref().is_some_and(|(card,_)|card==&id.get_value())) on:dragstart=move |event:leptos::ev::DragEvent|{
                            if busy(){event.prevent_default();return;}
                            if let Some(card)=current(){if let Some(transfer)=event.data_transfer(){if transfer.set_data("text/plain",&card.id).is_err(){event.prevent_default();return;}transfer.set_effect_allowed("move");}dragging.set(Some((card.id,field(&card.fields,"status"))));}
                        } on:dragend=move |_|dragging.set(None)>
                            <div class="card-actions"><button type="button" id=format!("card-actions-toggle-{}",id.get_value()) class="card-actions-toggle" popovertarget=format!("card-actions-{}",id.get_value()) aria-label=move ||i18n.text("Actions de la tâche") title=move ||i18n.text("Actions de la tâche")><svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><circle cx="3" cy="8" r="1.5"/><circle cx="8" cy="8" r="1.5"/><circle cx="13" cy="8" r="1.5"/></svg></button><div id=format!("card-actions-{}",id.get_value()) class="card-action-menu" popover="auto">
                                <button type="button" class="danger" hidden=move ||!running() disabled=move ||pending.get() popovertarget=format!("card-actions-{}",id.get_value()) popovertargetaction="hide" on:click=move |_|card_action.run((id.get_value(),"stop"))><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2"/></svg>{move ||i18n.text("Arrêter l’agent")}</button>
                                <button type="button" hidden=running disabled=busy popovertarget=format!("card-actions-{}",id.get_value()) popovertargetaction="hide" on:click=move |_|card_action.run((id.get_value(),"archive"))><NavigationIcon kind="archives" view_icon=true />{move ||i18n.text("Archiver")}</button>
                                <button type="button" class="danger" hidden=running disabled=busy popovertarget=format!("card-actions-{}",id.get_value()) popovertargetaction="hide" on:click=move |_|card_action.run((id.get_value(),"delete"))><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/></svg>{move ||i18n.text("Supprimer localement")}</button>
                            </div></div>
                            <span class="card-label">{move || current().and_then(|card|card.fields.get::<atelier_core::document::Document>("ticket").ok().flatten()).map(|ticket|field(&ticket,"key")).filter(|key|!key.is_empty()).unwrap_or_else(||i18n.text("TÂCHE LOCALE"))}</span>
                            <button type="button" class="card-title card-open" id=format!("card-open-{}", id.get_value()) on:click=move |_|open_task.run(id.get_value())>{move || current().map(|card| String::from_utf16_lossy(&card.title.0)).unwrap_or_default()}</button>
                            <span class="card-description">{move || current().map(|card| field(&card.fields, "description")).unwrap_or_default()}</span>
                            {move || current().and_then(|card|card.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist").ok().flatten()).filter(|criteria|!criteria.is_empty()).map(|criteria|view!{<span class="card-criteria">{i18n.text("{0}/{1} critères vérifiés").replace("{0}",&criteria.iter().filter(|criterion|criterion.done).count().to_string()).replace("{1}",&criteria.len().to_string())}</span>})}
                            <div class="card-bottom">{move || current().and_then(|card|{let agent=field(&card.fields,"agent");board.settings.with(|settings|atelier_core::domain::agent_options::providers(settings).unwrap_or_default().into_iter().find(|profile|field(profile,"id")==agent))}).map(|profile|view!{<span class="agent-pill">{field(&profile,"name")}</span>})}<span class="project-pill">{move || current().map(|card|{let repo=field(&card.fields,"repo");projects.with(|projects|projects.iter().find(|project|field(project,"repo")==repo).map(|project|field(project,"name"))).unwrap_or_else(||if repo.is_empty(){i18n.text("Sans projet")}else{repo.rsplit(['/', '\\']).find(|part|!part.is_empty()).unwrap_or(&repo).to_owned()})}).unwrap_or_default()}</span>
                                <span class="card-live" hidden=move || !board.runs.with(|runs|runs.iter().any(|run|field(run,"cardId")==id.get_value()&&field(run,"status")=="running"))>{move ||i18n.text("Travail en cours")}</span>
                            </div>
                        </article> }
                    } /><p class="empty" hidden=move || !column().is_empty() || loading.get()>{move || i18n.text("Rien pour le moment")}</p><button type="button" class="column-add" hidden=status!="todo" aria-label=move ||i18n.text("Ajouter une tâche") on:click=move |_|open_new.run(())>"＋"</button></section> }
                } />
            </section>
            <Show when=move || matches!(view.get(),WorkspaceView::Overview|WorkspaceView::Agents|WorkspaceView::Processes|WorkspaceView::Worktrees)><workspace_views::WorkspaceViews board token view=Signal::derive(move ||view.get()) project=Signal::derive(move ||repository.get()) on_open=open_task on_view=navigate on_focus=Callback::new(move |filter:String|{focus_filter.set(filter);search.set(String::new());navigate.run(WorkspaceView::Board);}) on_projects=Callback::new(move |()|show_projects.set(true)) /></Show>
            <Show when=move || view.get()==WorkspaceView::Archives><section id="workspace-archives" class="workspace-panel"><div class="panel-heading"><div><h2>{move ||i18n.text("Votre historique")}</h2><p class="hint">{move ||i18n.text("Les tâches archivées, avec leurs temps, branches et journaux. Restaurer les remet dans le tableau à leur étape précédente.")}</p></div></div><For each=move ||visible(false) key=|card|card.id.clone() children=move |card|{let id=StoredValue::new(card.id.clone());view!{<article class="workspace-row archive-row" data-id=id.get_value()><div><span class="section-kicker">{move ||i18n.text("Tâche archivée")}</span><h3>{String::from_utf16_lossy(&card.title.0)}</h3><p class="hint">{move ||i18n.text("Historique conservé")}</p></div><div class="row-actions"><button id=format!("card-open-{}",id.get_value()) class="card-open" on:click=move |_|open_task.run(id.get_value())>{move ||i18n.text("Ouvrir la tâche ↗")}</button><button disabled=move ||pending.get() on:click=move |_|{if pending.get_untracked(){return;}let Ok(token)=token.get_value()else{return;};pending.set(true);spawn_local(async move{let result=write(&token,Method::POST,&format!("/api/cards/{}/archive",id.get_value()),&serde_json::json!({"archived":false})).await;match result{Ok(_)=>{let _=board.refresh(&token).await;},Err(message)=>error.set(message)}pending.set(false);});}>{move ||i18n.text("Restaurer")}</button></div></article>}} /><p class="empty" hidden=move ||!visible(false).is_empty()>{move ||i18n.text("Aucune tâche archivée.")}</p></section></Show>
            <Show when=move ||view.get()==WorkspaceView::Brainstorm><BrainstormPage board token on_open=open_task on_new=open_new /></Show>
            {move || selected.get().filter(|_|view.get()==WorkspaceView::Task).map(|card| view! { <TaskEditor card board token on_open=open_task on_close=Callback::new(move |()| commit_view.run(return_view.get_untracked())) /> })}
            {move || match view.get(){WorkspaceView::Settings|WorkspaceView::Updates=>Some(view!{<Settings board token initial_tab=if view.get()==WorkspaceView::Updates{"updates"}else{settings_section.get_untracked()} on_close=Callback::new(move |()|commit_view.run(WorkspaceView::Overview)) />}),_=>None}}
            <Show when=move ||view.get()==WorkspaceView::Time><crate::time_report::TimeReport board token loading=Memo::new(move |_|loading.get()) load_error=Signal::derive(move ||if board.loaded.get(){String::new()}else{error.get()}) on_close=Callback::new(move |()|commit_view.run(WorkspaceView::Overview)) on_task=open_task /></Show>
            <footer>"ATELIER "<span>{move ||i18n.text("Un peu moins de friction. Un peu plus de fait.")}</span><span>{move ||i18n.text("Stockage local · SQLite")}</span></footer>
            <Show when=move || show_projects.get()><Projects board repository token on_close=Callback::new(move |()| show_projects.set(false)) /></Show>
            <Show when=move || show_agents.get()><AgentProfiles board token on_close=Callback::new(move |()| show_agents.set(false)) /></Show>
            <Show when=move || show_backups.get()><Backups board token on_close=Callback::new(move |()| show_backups.set(false)) /></Show>
            {move ||deleting_card.get().map(|card|{let id=card.id.clone();view!{<DeleteCardDialog card pending error on_confirm=Callback::new(move |()|card_action.run((id.clone(),"confirm-delete"))) on_close=Callback::new(move |()|deleting_card.set(None)) />}})}
            <Show when=move ||show_diagnostics.get()><DiagnosticsDialog board token on_close=Callback::new(move |()|show_diagnostics.set(false)) /></Show>
            <Show when=move ||command_open.get()><CommandPalette board on_close=Callback::new(move |()|command_open.set(false)) on_open=open_task on_new=open_new on_project=Callback::new(move |repo:String|{repository.set(repo);search.set(String::new());focus_filter.set("all".into());navigate.run(WorkspaceView::Overview);}) on_projects=Callback::new(move |()|show_projects.set(true)) on_diagnostics=Callback::new(move |()|show_diagnostics.set(true)) /></Show>
        </main>
            <dialog id="new-dialog" node_ref=new_dialog aria-label=move ||i18n.text("Nouvelle tâche") on:cancel=move |event:web_sys::Event|{event.prevent_default();close_new();}><form id="new-form" on:submit=move |event:leptos::ev::SubmitEvent| {
                event.prevent_default();
                if pending.get_untracked() || template_pending.get_untracked() || template_candidate.get_untracked().is_some() { return; }
                let Ok(token) = token.get_value() else { return; };
                let entered = title.get_untracked();
                let repo = new_repo.get_untracked();
                let selected_template = template.get_untracked();
                pending.set(true); create_error.set(String::new());
                spawn_local(async move {
                    let operation = async {
                        let mut body = serde_json::json!({"title":entered,"mode":new_mode.get_untracked(),"model":new_model.get_untracked(),"effort":new_effort.get_untracked()});
                        if !description.get_untracked().is_empty() {body["description"]=serde_json::json!(description.get_untracked());}
                        if !instructions.get_untracked().is_empty() {body["instructions"]=serde_json::json!(instructions.get_untracked());}
                        if !new_agent.get_untracked().is_empty() {body["agent"]=serde_json::json!(new_agent.get_untracked());}
                        if !repo.is_empty() { body["repo"] = serde_json::json!(&repo); }
                        if !selected_template.is_empty() { body["templateId"] = serde_json::json!(selected_template); }
                        let response = write(&token, Method::POST, "/api/cards", &body).await?;
                        let card: Card = response.json().await.map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        let id=card.id.clone(); replace_card(cards, card); new_initialized.set(false); title.set(String::new()); description.set(String::new()); instructions.set(String::new()); template.set(String::new()); new_model.set(String::new()); new_effort.set(String::new()); new_mode.set("edit".into()); if let Some(dialog)=new_dialog.get_untracked(){dialog.close();} focus_filter.set("all".into()); search.set(String::new()); repository.set(repo); view.set(WorkspaceView::Board); let _ = board.refresh(&token).await; open_task.run(id); Ok::<(), String>(())
                    }.await;
                    if let Err(message) = operation { create_error.set(message); }
                    pending.set(false);
                });
            }>
                <div class="dialog-heading"><div><div class="eyebrow">{move ||i18n.text("UNE CHOSE À LA FOIS")}</div><h2>{move ||i18n.text("Nouvelle tâche")}</h2></div><button type="button" aria-label=move ||i18n.text("Fermer") disabled=move ||pending.get() on:click=move |_|close_new()>"×"</button></div>
                <div class="template-options" aria-label=move ||i18n.text("Modèles de tâche")><span>{move ||i18n.text("Partir de :")}</span>{[("feature","Fonctionnalité"),("bug","Correction"),("analysis","Analyse")].into_iter().map(move |(kind,label)|view!{<button type="button" data-template=kind disabled=move ||pending.get() on:click=move |_| {
                    let proposed=task_template(kind);
                    let previous=description.get_untracked();
                    if !previous.trim().is_empty() && !["feature","bug","analysis"].into_iter().any(|kind|i18n.text(task_template(kind))==previous){create_error.set("Le contexte déjà saisi est conservé. Videz-le pour appliquer un modèle.".into());return;}
                    template.set(String::new());description.set(i18n.text(proposed));new_mode.set(if kind=="analysis"{"analyze"}else{"edit"}.into());focus_element("task-title");
                }>{move ||i18n.text(label)}</button>}).collect_view()}</div>
                <details class="disclosure" hidden=move || templates.get().is_empty()><summary>{move ||i18n.text("Utiliser un modèle personnel")}</summary><div class="template-picker"><label for="task-template">{move || i18n.text("Modèle")}</label><select id="task-template" disabled=move || pending.get() || template_pending.get() prop:value=move || template.get() on:change=move |event| { let id=event_target_value(&event);remove_template.set(false);if id.is_empty(){template.set(id);}else if !description.get_untracked().trim().is_empty()||!instructions.get_untracked().trim().is_empty(){template_candidate.set(Some(id));}else{apply_template.run(id);} }><option value="">{move || i18n.text("Sans modèle")}</option><For each=move || templates.get() key=|item| field(item,"id") children=|item| view! { <option value=field(&item,"id")>{field(&item,"name")}</option> } /></select><button id="remove-template" type="button" class="text-action" hidden=move || template.get().is_empty() disabled=move || pending.get() || template_pending.get() on:click=move |_| remove_template.set(true)>{move || i18n.text("Retirer ce modèle…")}</button></div></details>
                <section class="form-section"><div class="section-heading"><span class="section-number">"01"</span><div><h3>{move ||i18n.text("Le résultat attendu")}</h3></div></div><label for="task-title">{move ||i18n.text("Titre (obligatoire)")}<input node_ref=title_input id="task-title" name="title" required maxlength="300" disabled=move ||pending.get() placeholder=move ||i18n.text("Ex. Ajouter un export CSV des tâches") prop:value=move ||title.get() on:input=move |event|title.set(event_target_value(&event)) /></label><label>{move ||i18n.text("Description")}<textarea id="new-description" name="description" rows="5" maxlength="20000" disabled=move ||pending.get() placeholder=move ||i18n.text("Contexte, résultat attendu, contraintes…") prop:value=move ||description.get() on:input=move |event|description.set(event_target_value(&event)) /></label></section>
                <section class="form-section execution-options"><div class="section-heading"><span class="section-number">"02"</span><div><h3>{move ||i18n.text("Projet & agent")}</h3></div></div><label>{move ||i18n.text("Projet enregistré")}<select id="new-project" disabled=move ||pending.get() prop:value=move ||new_repo.get() on:change=move |event|{let repo=event_target_value(&event); if let Some(agent)=projects.with_untracked(|items|items.iter().find(|project|field(project,"repo")==repo).map(|project|field(project,"agent"))).filter(|agent|!agent.is_empty()){new_agent.set(agent);}new_repo.set(repo);}><option value="">{move ||i18n.text("Choisir un projet…")}</option><For each=move ||projects.get() key=|project|field(project,"id") children=move |project|view!{<option value=field(&project,"repo")>{field(&project,"name")}</option>} /></select></label><details class="disclosure"><summary>{move ||i18n.text("Choisir un autre répertoire")}</summary><label>{move ||i18n.text("Dépôt Git source")}<input name="repo" disabled=move ||pending.get() prop:value=move ||new_repo.get() on:input=move |event|new_repo.set(event_target_value(&event)) placeholder=move ||i18n.text("Chemin absolu du projet") /></label></details>
                <div class="form-row"><label>{move ||i18n.text("Agent")}<select name="agent" disabled=move ||pending.get() prop:value=move ||new_agent.get() on:change=move |event|{new_agent.set(event_target_value(&event));new_model.set(String::new());new_effort.set(String::new());}><option value="">{move ||i18n.text("Hériter du projet")}</option><For each=move ||atelier_core::domain::agent_options::providers(&board.settings.get()).unwrap_or_default() key=|profile|field(profile,"id") children=move |profile|view!{<option value=field(&profile,"id")>{field(&profile,"name")}</option>} /></select></label><label>{move ||i18n.text("Travail demandé")}<select name="mode" disabled=move ||pending.get() prop:value=move ||new_mode.get() on:change=move |event|new_mode.set(event_target_value(&event))><option value="edit">{move ||i18n.text("Modifier le code")}</option><option value="analyze">{move ||i18n.text("Analyser seulement")}</option></select></label></div><details class="disclosure"><summary>{move ||i18n.text("Ajouter des instructions complémentaires")}</summary><label>{move ||i18n.text("Instructions pour l’agent")}<textarea name="instructions" rows="2" maxlength="20000" disabled=move ||pending.get() prop:value=move ||instructions.get() on:input=move |event|instructions.set(event_target_value(&event)) /></label></details></section>
                <details class="disclosure model-options"><summary>{move ||i18n.text("Modèle & réflexion (facultatif)")}</summary><label>{move ||i18n.text("Modèle")}<input maxlength="180" disabled=move ||pending.get() prop:value=move ||new_model.get() on:input=move |event|new_model.set(event_target_value(&event)) /></label><label>{move ||i18n.text("Effort")}<input maxlength="40" disabled=move ||pending.get() prop:value=move ||new_effort.get() on:input=move |event|new_effort.set(event_target_value(&event)) /></label></details>
                <p class="hint notice-line">{move ||i18n.text("Créer la tâche ne lance pas l’agent. Vous pourrez relire ses instructions avant de démarrer.")}</p><p class="error" role="alert">{move ||i18n.text(&create_error.get())}</p><div class="form-actions"><button type="button" disabled=move ||pending.get() on:click=move |_|close_new()>{move ||i18n.text("Annuler")}</button><button id="create-task" class="primary" type="submit" disabled=move ||pending.get()||template_pending.get()||loading.get()||template_candidate.get().is_some()>{move ||i18n.text(if pending.get(){"Création…"}else{"Créer la tâche"})}</button></div>
            </form>
            <div class="confirmation" hidden=move ||template_candidate.get().is_none()><p>{move ||i18n.text("Remplacer la description, les instructions et le mode saisis par ce modèle ?")}</p><button type="button" disabled=move ||pending.get() on:click=move |_|{if let Some(id)=template_candidate.get_untracked(){apply_template.run(id);}}>{move ||i18n.text("Appliquer")}</button><button type="button" disabled=move ||pending.get() on:click=move |_|{template_candidate.set(None);template.set(String::new());}>{move ||i18n.text("Annuler")}</button></div>
            <div class="confirmation" hidden=move || !remove_template.get()><p>{move || i18n.text("Retirer ce modèle ? Les tâches existantes sont conservées.")}</p><button id="confirm-remove-template" type="button" disabled=move || template_pending.get() on:click=move |_| {
                if template_pending.get_untracked() { return; }
                let Ok(token) = token.get_value() else { return; };
                let id = template.get_untracked(); template_pending.set(true); create_error.set(String::new());
                spawn_local(async move {
                    match write(&token, Method::DELETE, &format!("/api/templates/{id}"), &serde_json::json!({})).await {
                        Ok(_) => { templates.update(|items| items.retain(|item| field(item,"id") != id)); template.set(String::new()); remove_template.set(false); let _ = board.refresh(&token).await; },
                        Err(message) => error.set(message),
                    }
                    template_pending.set(false);
                });
            }>{move || i18n.text("Retirer le modèle")}</button><button type="button" disabled=move || template_pending.get() on:click=move |_| remove_template.set(false)>{move || i18n.text("Annuler")}</button></div>

        </dialog>
    }
}

fn task_template(kind: &str) -> &'static str {
    match kind {
        "bug" => {
            "Problème observé\n\nÉtapes pour reproduire\n1. \n\nRésultat attendu\n\nVérification de la correction\n"
        }
        "analysis" => {
            "Question à examiner\n\nContexte\n\nLivrable attendu\nUne analyse avec les constats, les pistes et leurs limites. Ne pas modifier les fichiers.\n"
        }
        _ => "Objectif\n\nComportement attendu\n\nCritères de réussite\n- \n\nContraintes\n",
    }
}

fn matches_focus(
    card: &Card,
    cards: &[Card],
    runs: &[atelier_core::document::Document],
    filter: &str,
) -> bool {
    if filter == "review" {
        return field(&card.fields, "status") == "review";
    }
    if filter != "attention" {
        return true;
    }
    if field(&card.fields, "status") == "done" {
        return false;
    }
    let blocked = card
        .fields
        .get::<Vec<String>>("blockedBy")
        .ok()
        .flatten()
        .unwrap_or_default()
        .iter()
        .any(|id| {
            cards
                .iter()
                .find(|other| &other.id == id)
                .is_none_or(|other| field(&other.fields, "status") != "done")
        });
    let sync_error = card
        .fields
        .get::<atelier_core::document::Document>("ticketSync")
        .ok()
        .flatten()
        .is_some_and(|sync| field(&sync, "state") == "error");
    let automation = card
        .fields
        .get::<atelier_core::document::Document>("automation")
        .ok()
        .flatten()
        .is_some_and(|flow| {
            matches!(
                field(&flow, "state").as_str(),
                "error" | "interrupted" | "cancelled" | "blocked"
            )
        });
    let run_attention = runs
        .iter()
        .find(|run| field(run, "cardId") == card.id)
        .is_some_and(|run| {
            !field(run, "attention").is_empty()
                || matches!(
                    field(run, "status").as_str(),
                    "failed" | "interrupted" | "cancelled"
                )
                || run
                    .get::<Vec<serde_json::Value>>("interactions")
                    .ok()
                    .flatten()
                    .is_some_and(|items| !items.is_empty())
        });
    blocked || sync_error || automation || run_attention
}

#[component]
fn NavigationIcon(kind: &'static str, #[prop(default = false)] view_icon: bool) -> impl IntoView {
    let shape=match kind {
        "brainstorm"=>view!{<path d="M9 18h6m-6 3h6M8 14a7 7 0 1 1 8 0l-1 2H9z" />}.into_any(),
        "overview"=>view!{<rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" />}.into_any(),
        "projects"=>view!{<path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" /><path d="M3 10h18" />}.into_any(),
        "settings"=>view!{<path d="M4 7h16M4 17h16" /><circle cx="9" cy="7" r="3" /><circle cx="15" cy="17" r="3" />}.into_any(),
        "diagnostics"=>view!{<path d="M3 12h4l3-7 4 14 3-7h4" />}.into_any(),
        "backups"=>view!{<path d="M4 3h13l3 3v15H4Z" /><path d="M8 3v6h8V3M8 21v-8h8v8" />}.into_any(),
        "board"=>view!{<rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16M15 4v16" />}.into_any(),
        "agents"=>view!{<path d="M8 3v3m8-3v3M6 6h12v14H6zM3 10h3m12 0h3M3 16h3m12 0h3M9 10h6M9 15h6" />}.into_any(),
        "processes"=>view!{<path d="m9 5 11 7-11 7z" />}.into_any(),
        "worktrees"=>view!{<circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="5" r="2" /><path d="M6 7v10m12-10v3a5 5 0 0 1-5 5H6" />}.into_any(),
        "time"=>view!{<circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />}.into_any(),
        _=>view!{<path d="M4 8h16v12H4zM3 4h18v4H3zM9 12h6" />}.into_any(),
    };
    view! {<svg class=if view_icon{"view-icon"}else{"nav-icon"} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width=if view_icon{"1.6"}else{"1.7"} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">{shape}</svg>}
}

fn command_search(value: &str) -> String {
    String::from(js_sys::JsString::from(value).normalize("NFD"))
        .chars()
        .filter(|character| !('\u{0300}'..='\u{036f}').contains(character))
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, PartialEq)]
enum CommandTarget {
    New,
    Projects,
    Diagnostics,
    Project(String),
    Card(String),
}

#[component]
fn CommandPalette(
    board: BoardState,
    on_close: Callback<()>,
    on_open: Callback<String>,
    on_new: Callback<()>,
    on_project: Callback<String>,
    on_projects: Callback<()>,
    on_diagnostics: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let query = RwSignal::new(String::new());
    let index = RwSignal::new(0usize);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
        focus_element("command-query");
    });
    let results = Memo::new(move |_| {
        let mut items = vec![
            (
                CommandTarget::New,
                i18n.text("Créer une tâche"),
                i18n.text("Nouvelle tâche locale"),
            ),
            (
                CommandTarget::Projects,
                i18n.text("Gérer les projets"),
                i18n.text("Répertoires locaux"),
            ),
            (
                CommandTarget::Diagnostics,
                i18n.text("Tester les connexions"),
                i18n.text("Git, agents et services"),
            ),
        ];
        items.extend(board.projects.get().into_iter().map(|project| {
            (
                CommandTarget::Project(field(&project, "repo")),
                field(&project, "name"),
                field(&project, "repo"),
            )
        }));
        items.extend(board.cards.get().into_iter().map(|card| {
            let ticket = card
                .fields
                .get::<atelier_core::document::Document>("ticket")
                .ok()
                .flatten()
                .map(|ticket| field(&ticket, "key"))
                .unwrap_or_else(|| i18n.text("Tâche locale"));
            (
                CommandTarget::Card(card.id),
                String::from_utf16_lossy(&card.title.0),
                ticket,
            )
        }));
        let words = command_search(&query.get())
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        items
            .into_iter()
            .filter(|(_, title, detail)| {
                words
                    .iter()
                    .all(|word| command_search(&format!("{title} {detail}")).contains(word))
            })
            .take(30)
            .collect::<Vec<_>>()
    });
    let close = move || {
        if let Some(dialog) = dialog.get_untracked() {
            dialog.close();
        }
        on_close.run(());
        focus_element("command-button");
    };
    let execute = Callback::new(move |position: usize| {
        let target = results.with_untracked(|items| items.get(position).map(|item| item.0.clone()));
        if let Some(target) = target {
            close();
            match target {
                CommandTarget::New => on_new.run(()),
                CommandTarget::Projects => on_projects.run(()),
                CommandTarget::Diagnostics => on_diagnostics.run(()),
                CommandTarget::Project(repo) => on_project.run(repo),
                CommandTarget::Card(id) => on_open.run(id),
            }
        }
    });
    view! {<dialog id="command-dialog" aria-label=move ||i18n.text("Recherche rapide") node_ref=dialog on:cancel=move |event:web_sys::Event|{event.prevent_default();close();}><div class="dialog-heading"><div><div class="eyebrow">{move ||i18n.text("RECHERCHE RAPIDE")}</div><h2>{move ||i18n.text("Aller à…")}</h2></div><button type="button" aria-label=move ||i18n.text("Fermer") on:click=move |_|close()>"×"</button></div><label class="sr-only" for="command-query">{move ||i18n.text("Projet, tâche ou action")}</label><input id="command-query" type="search" autocomplete="off" placeholder=move ||i18n.text("Un ticket, un projet, une action…") prop:value=move ||query.get() on:input=move |event|{query.set(event_target_value(&event));index.set(0);} on:keydown=move |event:leptos::ev::KeyboardEvent|{match event.key().as_str(){"ArrowDown"=>{event.prevent_default();index.update(|index|*index=(*index+1).min(results.with(Vec::len).saturating_sub(1)));},"ArrowUp"=>{event.prevent_default();index.update(|index|*index=index.saturating_sub(1));},"Enter"=>{event.prevent_default();execute.run(index.get_untracked());},_=>{}}} /><div id="command-results" aria-live="polite">{move ||results.get().into_iter().enumerate().map(|(position,(target,title,detail))|view!{<button class="command-result" type="button" aria-current=move ||(index.get()==position).to_string() on:click=move |_|execute.run(position)><span class="command-kind">{i18n.text(match target{CommandTarget::Project(_)=>"PROJET",CommandTarget::Card(_)=>"TÂCHE",_=>"ACTION"})}</span><span><strong>{title}</strong><small>{detail}</small></span><span>"↵"</span></button>}).collect_view()}<p class="empty" hidden=move ||!results.with(Vec::is_empty)>{move ||i18n.text("Aucun résultat. Essayez un titre, un ID Jira ou un nom de projet.")}</p></div><p class="search-help"><kbd>"↑"</kbd><kbd>"↓"</kbd>{move ||i18n.text("parcourir")}<span><kbd>{move ||i18n.text("Entrée")}</kbd>{move ||i18n.text("ouvrir")}</span><span><kbd>{move ||i18n.text("Échap")}</kbd>{move ||i18n.text("fermer")}</span></p></dialog>}
}

#[component]
fn DiagnosticsDialog(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let pending = RwSignal::new(false);
    let profile = RwSignal::new(None::<atelier_core::document::Document>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if !pending.get_untracked() {
            if let Some(dialog) = dialog.get_untracked() {
                dialog.close();
            }
            on_close.run(());
            focus_element("diagnostics-button");
        }
    };
    view! {<dialog id="diagnostics-dialog" aria-label=move ||i18n.text("État des services") node_ref=dialog on:cancel=move |event:web_sys::Event|{event.prevent_default();close();}><div class="dialog-heading"><div><div class="eyebrow">{move ||i18n.text("CONNEXIONS")}</div><h2>{move ||i18n.text("État des services")}</h2></div><button disabled=move ||pending.get() aria-label=move ||i18n.text("Fermer") on:click=move |_|close()>"×"</button></div><p class="hint">{move ||i18n.text("Contrôle explicite du profil enregistré : version et état de connexion du CLI. Aucun prompt envoyé et aucune requête payante de validation de clé.")}</p><label>{move ||i18n.text("Agent")}<select disabled=move ||pending.get() on:change=move |event|{let id=event_target_value(&event);profile.set(atelier_core::domain::agent_options::providers(&board.settings.get_untracked()).unwrap_or_default().into_iter().find(|item|field(item,"id")==id));}><option value="">{move ||i18n.text("Choisir un profil")}</option><For each=move ||atelier_core::domain::agent_options::providers(&board.settings.get()).unwrap_or_default() key=|profile|field(profile,"id") children=move |profile|view!{<option value=field(&profile,"id")>{field(&profile,"name")}</option>} /></select></label><div id="diagnostics-results" aria-live="polite"><agent_diagnostics::AgentDiagnostics profile token pending blocked=Memo::new(|_|false) /></div></dialog>}
}

#[component]
fn DeleteCardDialog(
    card: Card,
    pending: RwSignal<bool>,
    error: RwSignal<String>,
    on_confirm: Callback<()>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card.id);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if pending.get_untracked() {
            return;
        }
        if let Some(dialog) = dialog.get_untracked() {
            dialog.close();
        }
        let target = format!("card-actions-toggle-{}", id.get_value());
        on_close.run(());
        request_animation_frame(move || {
            if let Some(element) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(&target))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        });
    };
    view! {<dialog id="confirm-dialog" class="confirm-dialog" role="alertdialog" aria-labelledby="confirm-title" aria-describedby="confirm-message" node_ref=dialog on:cancel=move |event:web_sys::Event|{event.prevent_default();close();}><form on:submit=move |event|{event.prevent_default();on_confirm.run(());}>
        <div class="dialog-heading"><div class="confirm-icon" aria-hidden="true"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M10.3 4.6 2.5 18a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 4.6a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4m0 4h.01"/></svg></div><button type="button" disabled=move ||pending.get() aria-label=move ||i18n.text("Fermer") on:click=move |_|close()>"×"</button></div>
        <h2 id="confirm-title">{move ||i18n.text("Supprimer définitivement cette tâche ?")}</h2><p>{String::from_utf16_lossy(&card.title.0)}</p><p id="confirm-message">{move ||i18n.text("Supprimer définitivement cette tâche ? Cette action ne peut pas être annulée.")}</p><p class="error" role="alert">{move ||i18n.text(&error.get())}</p><div class="form-actions"><button type="button" autofocus disabled=move ||pending.get() on:click=move |_|close()>{move ||i18n.text("Annuler")}</button><button id="confirm-action" class="confirm-action" type="submit" disabled=move ||pending.get()>{move ||i18n.text("Supprimer définitivement")}</button></div>
    </form></dialog>}
}
fn field(document: &atelier_core::document::Document, key: &str) -> String {
    document
        .get::<atelier_core::document::JsonString>(key)
        .ok()
        .flatten()
        .map(|value| String::from_utf16_lossy(&value.0))
        .unwrap_or_default()
}

fn main() {
    leptos::mount::mount_to_body(App);
}
