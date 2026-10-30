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
mod button;
mod card;
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
mod form;
mod git_push;
mod i18n;
mod integration_choice;
mod jira;
mod jira_creation;
mod jira_refresh;
mod jira_task;
mod kanban_settings;
mod live;
mod modal;
mod new_task;
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
mod ticket_sync;
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
    Projects,
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
            Self::Projects => "projects",
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
            "projects" => Self::Projects,
            "settings" => Self::Settings,
            "updates" => Self::Updates,
            _ => Self::Overview,
        }
    }
    fn heading(self) -> (&'static str, &'static str) {
        match self {
            Self::Projects => ("message.my_projects", "message.manage_project_settings"),
            Self::Settings | Self::Updates => (
                "message.settings",
                "message.general_settings_modules_and_connections",
            ),
            Self::Brainstorm => (
                "message.brainstorming",
                "message.explore_an_idea_external_tickets_optional",
            ),
            Self::Overview => ("message.overview", "message.your_next_actions_at_a_glance"),
            Self::Board => (
                "message.task_board",
                "message.prepare_start_verify_one_step_at_a_time",
            ),
            Self::Agents => (
                "message.agent_activity_b92876f1",
                "message.track_runs_and_find_their_results",
            ),
            Self::Processes => (
                "message.processes",
                "message.launch_and_control_your_projects_frontends_and_backends",
            ),
            Self::Worktrees => (
                "message.branches_pull_requests",
                "message.find_working_folders_branches_and_prs",
            ),
            Self::Time => (
                "message.time_spent",
                "message.review_your_entries_before_sending_them_to_the_connected_service",
            ),
            Self::Archives => (
                "message.archives",
                "message.find_your_history_and_restore_a_task_when_you_need_it",
            ),
            Self::Diagnostics => ("message.diagnostics", "message.test_connections"),
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
    ("todo", "message.to_do"),
    ("doing", "message.in_progress"),
    ("review", "message.to_review"),
    ("done", "message.done"),
    ("standby", "message.on_hold"),
];

async fn write(
    token: &str,
    method: Method,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response, String> {
    let deadline = path
        .ends_with("/diagnose")
        .then(|| web_sys::AbortSignal::timeout_with_u32(60_000));
    let send = || async {
        let response = RequestBuilder::new(path)
            .method(method.clone())
            .abort_signal(deadline.as_ref())
            .header("Authorization", &format!("Bearer {token}"))
            .header("X-Local-Request", "1")
            .json(body)
            .map_err(|_| "message.invalid_request")?
            .send()
            .await
            .map_err(|_| {
                if deadline.as_ref().is_some_and(|signal| signal.aborted()) {
                    "message.agent_diagnostic_timeout"
                } else {
                    "message.connection_interrupted_check_the_current_state_before_trying_aga"
                }
            })?;
        checked_response(response).await
    };
    retry_request(send, path.ends_with("/diagnose")).await
}

// Admission refuses before reservation. Other write failures can have an effect and must never retry.
fn request_retry_allowed(message: Option<&str>, diagnostic: bool) -> bool {
    matches!(
        message,
        Some("message.a_change_is_in_progress_try_again_shortly")
    ) || (diagnostic
        && matches!(
            message,
            Some(
                "message.agent_diagnostic_busy"
                    | "message.executable_lookup_is_busy_try_again_shortly"
            )
        ))
}

#[cfg(test)]
mod retry_policy_tests {
    #[test]
    fn writes_only_retry_a_refusal_before_any_effect() {
        assert!(super::request_retry_allowed(
            Some("message.a_change_is_in_progress_try_again_shortly"),
            false
        ));
        for message in [
            None,
            Some("message.agent_diagnostic_busy"),
            Some("message.operation_refused"),
            Some("message.connection_interrupted_check_the_current_state_before_trying_aga"),
        ] {
            assert!(!super::request_retry_allowed(message, false));
        }
        assert!(super::request_retry_allowed(
            Some("message.agent_diagnostic_busy"),
            true
        ));
    }
}

async fn retry_request<F, Fut>(mut send: F, diagnostic: bool) -> Result<Response, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Response, String>>,
{
    let attempts = if diagnostic { 60 } else { 15 };
    for attempt in 0..attempts {
        let result = send().await;
        if attempt + 1 == attempts
            || !request_retry_allowed(result.as_ref().err().map(String::as_str), diagnostic)
        {
            return result;
        }
        let pause = js_sys::Promise::new(&mut |resolve, _| {
            set_timeout(
                move || {
                    let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
                },
                std::time::Duration::from_millis(if diagnostic { 1000 } else { 200 }),
            );
        });
        let _ = wasm_bindgen_futures::JsFuture::from(pause).await;
    }
    unreachable!()
}

async fn checked_response(response: Response) -> Result<Response, String> {
    if !response.ok() {
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "message.invalid_server_response")?;
        return Err(body
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("message.operation_refused")
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
    let window = web_sys::window().ok_or("message.browser_unavailable")?;
    let location = window.location();
    let params = web_sys::UrlSearchParams::new_with_str(
        location
            .hash()
            .map_err(|_| "message.invalid_address")?
            .trim_start_matches('#'),
    )
    .map_err(|_| "message.invalid_address")?;
    let local = window
        .local_storage()
        .map_err(|_| "message.local_storage_unavailable")?;
    let tab = window
        .session_storage()
        .map_err(|_| "message.session_storage_unavailable")?;
    let token = params
        .get("session")
        .or_else(|| local.as_ref()?.get_item("atelier-session").ok()?)
        .or_else(|| tab.as_ref()?.get_item("atelier-session").ok()?)
        .ok_or("message.no_session_found_reopen_atelier_using_its_launcher")?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("message.invalid_session".into());
    }
    let address = format!(
        "{}{}",
        location.pathname().map_err(|_| "message.invalid_address")?,
        location.search().map_err(|_| "message.invalid_address")?,
    );
    window
        .history()
        .and_then(|history| history.replace_state_with_url(&history.state()?, "", Some(&address)))
        .map_err(|_| "message.navigation_unavailable")?;
    for storage in [local, tab].into_iter().flatten() {
        let _ = storage.set_item("atelier-session", &token);
    }
    Ok(token)
}

async fn read(token: &str, path: &str) -> Result<Response, String> {
    let deadline = (path == "/api/agent-providers/discover")
        .then(|| web_sys::AbortSignal::timeout_with_u32(60_000));
    let send = || async {
        let response = Request::get(path)
            .abort_signal(deadline.as_ref())
            .header("Authorization", &format!("Bearer {token}"))
            .send()
            .await
            .map_err(|_| "message.connection_interrupted")?;
        checked_response(response).await
    };
    retry_request(send, path == "/api/agent-providers/discover").await
}

async fn load(token: &str) -> Result<Board, String> {
    read(token, "/api/state")
        .await?
        .json::<Board>()
        .await
        .map_err(|_| "message.invalid_server_response".into())
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
    let sidebar_collapsed = RwSignal::new(
        web_sys::window()
            .and_then(|window| window.inner_width().ok())
            .and_then(|width| width.as_f64())
            .is_some_and(|width| width <= 800.0),
    );
    let sidebar_toggle_label = move || {
        i18n.text(if sidebar_collapsed.get() {
            "message.expand_navigation"
        } else {
            "message.collapse_navigation"
        })
    };
    let show_agents = RwSignal::new(false);
    let show_backups = RwSignal::new(false);
    let show_diagnostics = RwSignal::new(false);
    let deleting_card = RwSignal::new(None::<Card>);
    let dragging = RwSignal::new(None::<(String, String)>);
    let command_open = RwSignal::new(false);
    let settings_section = RwSignal::new(match requested_parameter("section").as_deref() {
        Some("tickets") => "tickets",
        Some("time") => "time",
        Some("forges") => "forges",
        Some("updates") => "updates",
        _ => "general",
    });
    let cards = board.cards;
    let projects = board.projects;
    let search = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let focus_filter = RwSignal::new("all".to_owned());
    let loading = RwSignal::new(true);
    let new_task_request = RwSignal::new(0u64);
    let error = RwSignal::new(String::new());
    let ticket_sync_notice = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let selected = RwSignal::new(None::<Card>);
    let show_kanban_settings = RwSignal::new(false);
    let token = StoredValue::new(session());
    let agent_status = RwSignal::new(atelier_core::application::agent_status::Checks::default());
    provide_context(agent_diagnostics::AgentStatuses(agent_status));
    let agent_checking = RwSignal::new(true);
    let agent_check_error = RwSignal::new(String::new());
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
            .ok_or("message.browser_unavailable".to_owned())
            .and_then(|window| {
                window
                    .history()
                    .map_err(|_| "message.navigation_unavailable".to_owned())
            })
            .and_then(|history| {
                history
                    .push_state_with_url(
                        &wasm_bindgen::JsValue::from_f64(next as f64),
                        "",
                        Some(&path),
                    )
                    .map_err(|_| "message.navigation_unavailable".to_owned())
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
                        let agent_token = token.clone();
                        spawn_local(async move {
                            match agent_profiles::discover_profiles(&agent_token, board).await {
                                Ok((profiles, _)) => {
                                    let mut details = Vec::new();
                                    for profile in profiles.into_iter().filter(|profile| atelier_core::domain::agent_catalog::find(&field(profile, "type")).is_some_and(|agent| agent.protocol != atelier_core::domain::agent_catalog::ProtocolKind::Command)) {
                                        let id = field(&profile, "id");
                                        let mut status_request = 0;
                                        agent_status.update(|checks| status_request = checks.begin(&id, &profile));
                                        let path = format!("/api/agent-providers/{id}/diagnose");
                                        let result = match write(&agent_token, Method::POST, &path, &serde_json::json!({"expected":profile,"statusOnly":true})).await {
                                            Ok(response) => response.json::<atelier_core::contracts::AgentDiagnostic>().await.ok(),
                                            Err(_) => None,
                                        };
                                        if agent_status.try_update(|statuses| { statuses.finish(&id, status_request, result); }).is_none() { return; }
                                        details.push((id, profile, status_request));
                                    }
                                    let _ = agent_checking.try_set(false);
                                    for (id, profile, status_request) in details {
                                        let path = format!("/api/agent-providers/{id}/diagnose");
                                        if let Ok(response) = write(
                                            &agent_token,
                                            Method::POST,
                                            &path,
                                            &serde_json::json!({"expected":profile}),
                                        )
                                        .await
                                            && let Ok(diagnostic) = response
                                                .json::<atelier_core::contracts::AgentDiagnostic>()
                                                .await
                                            && agent_status
                                                .try_update(|checks| {
                                                    checks.finish(
                                                        &id,
                                                        status_request,
                                                        Some(diagnostic),
                                                    )
                                                })
                                                .is_none()
                                        {
                                            return;
                                        }
                                    }
                                }
                                Err(message) => {
                                    let _ = agent_check_error.try_set(message);
                                }
                            }
                            let _ = agent_checking.try_set(false);
                        });
                        let requested = requested_task();
                        let active = requested.as_ref().and_then(|id| {
                            cards.with_untracked(|cards| {
                                cards.iter().find(|card| &card.id == id).cloned()
                            })
                        });
                        if requested.is_some() && active.is_none() {
                            error.set(
                                "message.this_task_was_not_found_you_can_still_use_the_board"
                                    .into(),
                            );
                            view.set(WorkspaceView::Board);
                        }
                        selected.set(active);
                    }
                    Err(message) => {
                        error.set(message);
                        agent_checking.set(false);
                    }
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
                card.fields.flag("brainstormSession").unwrap_or(false) == brainstorms
                    && card.fields.flag("archived").unwrap_or(false)
                        == (view.get() == WorkspaceView::Archives)
                    && (repo.is_empty() || field(&card.fields, "repo") == repo)
                    && (query.is_empty()
                        || format!(
                            "{} {} {}",
                            card.title.clone(),
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
    let onboarding = move || {
        matches!(view.get(), WorkspaceView::Board | WorkspaceView::Overview)
            && !loading.get()
            && !cards.with(|cards| {
                cards
                    .iter()
                    .any(|card| !card.fields.flag("brainstormSession").unwrap_or(false))
            })
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
                error.set("message.this_task_was_not_found_you_can_still_use_the_board".into());
                return;
            }
            if view.get_untracked() != WorkspaceView::Task {
                return_view.set(view.get_untracked());
            }
            selected.set(card);
            view.set(WorkspaceView::Task);
            focus_element("editor-heading");
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
                    | WorkspaceView::Projects
                    | WorkspaceView::Updates
                    | WorkspaceView::Brainstorm
            ) {
                commit_view.run(WorkspaceView::Board);
            }
            new_task_request.update(|request| *request += 1);
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
                    Err(_) => error.set("message.invalid_server_response".into()),
                },
                Err(message) => error.set(message),
            }
            let _ = board.refresh(&token).await;
            pending.set(false);
        });
    });
    let workspace_controls = move || {
        !matches!(
            view.get(),
            WorkspaceView::Settings
                | WorkspaceView::Projects
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
                    error.set("message.this_task_was_not_found_you_can_still_use_the_board".into());
                }
            } else {
                selected.set(None);
                settings_section.set(match requested_parameter("section").as_deref() {
                    Some("tickets") => "tickets",
                    Some("time") => "time",
                    Some("forges") => "forges",
                    Some("updates") => "updates",
                    _ => "general",
                });
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
                    error.set("message.navigation_unavailable".into());
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
                error.set("message.navigation_unavailable".into());
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
    view! {
        <crate::button::Button
            class="skip-link"
            {..}
            type="button"
            on:click={move |_| focus_element("main-content")}
        >
            {move || i18n.text("message.skip_to_content")}
        </crate::button::Button>
        <aside
            id="main-navigation"
            class="sidebar"
            class:sidebar-collapsed={move || sidebar_collapsed.get()}
            aria-label={move || i18n.text("message.main_navigation")}
        >
            <div class="sidebar-header">
                <a
                    class="brand"
                    href="/overview"
                    aria-label={move || i18n.text("message.atelier_kanban_home")}
                    on:click={move |event| {
                        event.prevent_default();
                        navigate.run(WorkspaceView::Overview);
                    }}
                >
                    <img class="brand-logo" src="/atelier-kanban.png" width="40" height="40" alt="" />
                    <span>"Atelier"<span class="brand-subtitle">"Kanban"</span></span>
                    <small class="brand-version" id="app-version">
                        {env!("CARGO_PKG_VERSION")}
                    </small>
                </a>
                <crate::button::Button
                    class="sidebar-toggle"
                    {..}
                    id="toggle-navigation"
                    type="button"
                    aria-controls="main-navigation"
                    aria-expanded={move || (!sidebar_collapsed.get()).to_string()}
                    aria-label={sidebar_toggle_label}
                    title={sidebar_toggle_label}
                    on:click={move |_| sidebar_collapsed.update(|collapsed| *collapsed = !*collapsed)}
                >
                    <NavigationIcon kind="collapse" />
                    <NavigationIcon kind="menu" />
                </crate::button::Button>
            </div>
            <nav class="sidebar-primary" aria-label={move || i18n.text("message.personal_workspace")}>
                <crate::button::Button
                    class="nav-item"
                    {..}
                    class:nav-active={move || view.get() == WorkspaceView::Brainstorm}
                    data-view="brainstorm"
                    id="brainstorm-button"
                    aria-current={move || {
                        if view.get() == WorkspaceView::Brainstorm { "page" } else { "false" }
                    }}
                    on:click={move |_| navigate.run(WorkspaceView::Brainstorm)}
                    title={move || i18n.text("message.brainstorming")}
                >
                    <NavigationIcon kind="brainstorm" />
                    <span class="nav-label">{move || i18n.text("message.brainstorming")}</span>
                </crate::button::Button>
                <crate::button::Button
                    class="nav-item"
                    {..}
                    class:nav-active={move || view.get() == WorkspaceView::Overview}
                    id="overview-button"
                    aria-current={move || {
                        if view.get() == WorkspaceView::Overview { "page" } else { "false" }
                    }}
                    on:click={move |_| navigate.run(WorkspaceView::Overview)}
                    title={move || i18n.text("message.overview")}
                >
                    <NavigationIcon kind="overview" />
                    <span class="nav-label">{move || i18n.text("message.overview")}</span>
                </crate::button::Button>
                <crate::button::Button
                    disabled={Signal::derive(move || loading.get())}
                    class="nav-item"
                    {..}
                    id="manage-projects"
                    type="button"
                    class:nav-active={move || view.get() == WorkspaceView::Projects}
                    aria-current={move || {
                        if view.get() == WorkspaceView::Projects { "page" } else { "false" }
                    }}
                    on:click={move |_| navigate.run(WorkspaceView::Projects)}
                    title={move || i18n.text("message.my_projects")}
                >
                    <NavigationIcon kind="projects" />
                    <span class="nav-label">{move || i18n.text("message.my_projects")}</span>
                </crate::button::Button>
            </nav>
            <section class="sidebar-agents">
                <div class="workspace-label">{move || i18n.text("message.your_agents")}</div>
                <div class="sidebar-agent-container">
                    <div class="sidebar-agent-list">
                        <For
                            each={move || {
                                atelier_core::domain::agent_options::providers(&board.settings.get())
                                    .unwrap_or_default()
                            }}
                            key={|profile| field(profile, "id")}
                            children={move |profile| {
                                let profile_title = field(&profile, "name");
                                view! {
                                    <crate::button::Button
                                        class="agent-row"
                                        {..}
                                        type="button"
                                        title={profile_title}
                                        on:click={move |_| show_agents.set(true)}
                                    >
                                        <span class="agent-avatar">
                                            {field(&profile, "name").chars().next().unwrap_or('A')}
                                        </span>
                                        <span class="agent-row-copy">
                                            <span class="agent-status">
                                                <span
                                                    class={
                                                        let id = field(&profile, "id");
                                                        move || {
                                                            if agent_diagnostics::current_status(
                                                                    agent_status,
                                                                    &id,
                                                                    &board.settings.get(),
                                                                )
                                                                .is_some_and(|status| status.cli_connected())
                                                            {
                                                                "online-dot agent-status-online"
                                                            } else {
                                                                "online-dot offline"
                                                            }
                                                        }
                                                    }
                                                    aria-hidden="true"
                                                ></span>
                                                <span class="sr-only">
                                                    {
                                                        let id = field(&profile, "id");
                                                        move || {
                                                            i18n.text(
                                                                if agent_checking.get()
                                                                    && agent_diagnostics::current_status(
                                                                            agent_status,
                                                                            &id,
                                                                            &board.settings.get(),
                                                                        )
                                                                        .is_none()
                                                                {
                                                                    "message.checking"
                                                                } else {
                                                                    match agent_diagnostics::current_status(
                                                                            agent_status,
                                                                            &id,
                                                                            &board.settings.get(),
                                                                        )
                                                                        .map(|status| (
                                                                            status.authentication,
                                                                            status.account_report.map(|report| report.state),
                                                                        ))
                                                                    {
                                                                        Some(
                                                                            (
                                                                                _,
                                                                                Some(
                                                                                    atelier_core::application::agent_account::State::Available,
                                                                                ),
                                                                            ),
                                                                        ) => {
                                                                            "message.limits_reported_by_the_service_they_do_not_establish_access_to_a"
                                                                        }
                                                                        Some(
                                                                            (
                                                                                _,
                                                                                Some(
                                                                                    atelier_core::application::agent_account::State::Disconnected,
                                                                                ),
                                                                            ),
                                                                        ) => "message.sign_in_through_the_cli_then_check_again",
                                                                        Some(
                                                                            (
                                                                                _,
                                                                                Some(
                                                                                    atelier_core::application::agent_account::State::RateLimited,
                                                                                ),
                                                                            ),
                                                                        ) => {
                                                                            "message.the_service_is_limiting_reads_wait_until_the_next_permitted_atte"
                                                                        }
                                                                        Some(
                                                                            (atelier_core::contracts::AgentAuthentication::Present, _),
                                                                        ) => {
                                                                            "message.cli_login_present_service_access_has_not_been_verified"
                                                                        }
                                                                        Some(
                                                                            (atelier_core::contracts::AgentAuthentication::Absent, _),
                                                                        ) => "message.no_cli_login",
                                                                        Some(
                                                                            (
                                                                                atelier_core::contracts::AgentAuthentication::Configured,
                                                                                _,
                                                                            ),
                                                                        ) => "message.key_present_validity_has_not_been_verified",
                                                                        Some(
                                                                            (
                                                                                atelier_core::contracts::AgentAuthentication::MissingKey,
                                                                                _,
                                                                            ),
                                                                        ) => "message.no_saved_api_key",
                                                                        _ => "message.not_verified",
                                                                    }
                                                                },
                                                            )
                                                        }
                                                    }
                                                </span>
                                            </span>
                                            <strong>{field(&profile, "name")}</strong>
                                        </span>
                                        <span class="agent-chevron" aria-hidden="true">
                                            "›"
                                        </span>
                                    </crate::button::Button>
                                }
                            }}
                        />
                        <p class="error" role="status">
                            {move || i18n.text(&agent_check_error.get())}
                        </p>
                    </div>
                    <crate::button::Button
                        variant={crate::button::ButtonVariant::Ghost}
                        disabled={Signal::derive(move || loading.get())}
                        class="text-button"
                        {..}
                        id="manage-agents"
                        title={move || i18n.text("message.manage_agents")}
                        type="button"
                        on:click={move |_| show_agents.set(true)}
                    >
                        <NavigationIcon kind="agents" />
                        <span class="nav-label">{move || i18n.text("message.manage_agents")}</span>
                    </crate::button::Button>
                </div>
            </section>
            <div class="sidebar-bottom">
                <nav class="sidebar-utilities" aria-label={move || i18n.text("message.settings_4ed117e0")}>
                    <crate::button::Button
                        disabled={Signal::derive(move || loading.get())}
                        class="nav-item"
                        {..}
                        id="manage-settings"
                        type="button"
                        class:nav-active={move || view.get() == WorkspaceView::Settings}
                        aria-current={move || {
                            if view.get() == WorkspaceView::Settings { "page" } else { "false" }
                        }}
                        on:click={move |_| {
                            settings_section.set("general");
                            navigate.run(WorkspaceView::Settings);
                        }}
                        title={move || i18n.text("message.settings_4ed117e0")}
                    >
                        <NavigationIcon kind="settings" />
                        <span class="nav-label">{move || i18n.text("message.settings_4ed117e0")}</span>
                    </crate::button::Button>
                    <crate::button::Button
                        disabled={Signal::derive(move || loading.get())}
                        class="nav-item"
                        {..}
                        id="diagnostics-button"
                        type="button"
                        on:click={move |_| show_diagnostics.set(true)}
                        title={move || i18n.text("message.diagnostics")}
                    >
                        <NavigationIcon kind="diagnostics" />
                        <span class="nav-label">{move || i18n.text("message.diagnostics")}</span>
                    </crate::button::Button>
                    <crate::button::Button
                        disabled={Signal::derive(move || loading.get())}
                        class="nav-item"
                        {..}
                        id="manage-backups"
                        type="button"
                        on:click={move |_| show_backups.set(true)}
                        title={move || i18n.text("message.backups")}
                    >
                        <NavigationIcon kind="backups" />
                        <span class="nav-label">{move || i18n.text("message.backups")}</span>
                    </crate::button::Button>
                    <crate::button::Button
                        disabled={Signal::derive(move || loading.get())}
                        class="nav-item"
                        {..}
                        id="check-updates"
                        type="button"
                        class:nav-active={move || view.get() == WorkspaceView::Updates}
                        aria-current={move || {
                            if view.get() == WorkspaceView::Updates { "page" } else { "false" }
                        }}
                        on:click={move |_| navigate.run(WorkspaceView::Updates)}
                        title={move || i18n.text("message.updates")}
                    >
                        <NavigationIcon kind="updates" />
                        <span class="nav-label">{move || i18n.text("message.updates")}</span>
                    </crate::button::Button>
                </nav>
                <div class="local-note">
                    <small>{move || i18n.text("message.your_cards_stay_here")}</small>
                </div>
            </div>
        </aside>
        <main id="main-content" tabindex="-1">
            <header class="topbar">
                <span class="topbar-context">
                    <span class="topbar-workspace">{move || i18n.text("message.my_workspace")}</span>
                    <span class="slash" aria-hidden="true">
                        "/"
                    </span>
                    <strong id="breadcrumb">
                        {move || {
                            projects
                                .with(|items| {
                                    items
                                        .iter()
                                        .find(|project| field(project, "repo") == repository.get())
                                        .map(|project| field(project, "name"))
                                })
                                .unwrap_or_else(|| i18n.text("message.all_projects"))
                        }}
                    </strong>
                </span>
                <crate::button::Button
                    class="command-trigger"
                    {..}
                    id="command-button"
                    type="button"
                    aria-haspopup="dialog"
                    aria-keyshortcuts="Control+k Meta+k"
                    on:click={move |_| command_open.set(true)}
                >
                    <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        aria-hidden="true"
                    >
                        <circle cx="10.5" cy="10.5" r="6.5" />
                        <path d="m16 16 4 4" />
                    </svg>
                    <span>{move || i18n.text("message.search_go_to")}</span>
                    <kbd>"Ctrl K"</kbd>
                </crate::button::Button>
                <span class="topbar-local" title={move || i18n.text("message.your_cards_stay_here")}>
                    "Local"
                </span>
            </header>
            <section class="heading" hidden={move || view.get() == WorkspaceView::Task}>
                <div>
                    <h1 id="workspace-title">{move || i18n.text(view.get().heading().0)}</h1>
                    <p id="workspace-subtitle">{move || i18n.text(view.get().heading().1)}</p>
                </div>
                <div class="heading-actions" hidden={move || !workspace_controls() || onboarding()}>
                    <crate::button::Button
                        variant={crate::button::ButtonVariant::Primary}
                        disabled={Signal::derive(move || pending.get() || loading.get())}
                        class="primary"
                        {..}
                        id="create-button"
                        type="button"
                        hidden={move || !workspace_controls() || onboarding()}
                        on:click={move |_| open_new.run(())}
                    >
                        "＋ "
                        {move || i18n.text("message.new_task")}
                        <kbd>"N"</kbd>
                    </crate::button::Button>
                </div>
            </section>
            <section class="toolbar" hidden={move || !workspace_controls()}>
                <nav class="tabs" aria-label={move || i18n.text("message.project_views")}>
                    {[
                        (WorkspaceView::Overview, "message.overview_bb28a9f4"),
                        (WorkspaceView::Board, "message.kanban"),
                        (WorkspaceView::Agents, "message.agents_279b44d2"),
                        (WorkspaceView::Processes, "message.processes"),
                        (WorkspaceView::Worktrees, "message.branches_prs"),
                        (WorkspaceView::Time, "message.time_47b15010"),
                        (WorkspaceView::Archives, "message.archives"),
                    ]
                        .into_iter()
                        .map(move |(target, label)| {
                            view! {
                                <crate::button::Button
                                    class="view-tab"
                                    {..}
                                    class:selected-tab={move || view.get() == target}
                                    data-view={target.key()}
                                    id={match target {
                                        WorkspaceView::Time => "view-time-report",
                                        WorkspaceView::Archives => "show-archives",
                                        _ => target.key(),
                                    }}
                                    aria-pressed={move || (view.get() == target).to_string()}
                                    on:click={move |_| navigate.run(target)}
                                >
                                    <NavigationIcon kind={target.key()} view_icon=true />
                                    <span>{move || i18n.text(label)}</span>
                                </crate::button::Button>
                            }
                        })
                        .collect_view()}
                </nav>
            </section>
            <section
                class="toolbar-actions"
                aria-label={move || i18n.text("message.project_and_search")}
                hidden={move || !workspace_controls()}
            >
                <crate::form::Field id="board-project" label=Signal::derive(move || i18n.text("message.project"))><crate::form::Select
                        {..}
                        id="board-project"
                        prop:value={move || repository.get()}
                        on:change={move |event| {
                            repository.set(event_target_value(&event));
                            focus_filter.set("all".into());
                            search.set(String::new());
                        }}
                    >
                        <option value="">{move || i18n.text("message.all_projects")}</option>
                        <For
                            each={move || projects.get()}
                            key={|project| field(project, "id")}
                            children={move |project| {
                                view! {
                                    <option value={field(&project, "repo")}>{field(&project, "name")}</option>
                                }
                            }}
                        />
                    </crate::form::Select></crate::form::Field>
                <crate::form::Input
                    {..}
                    id="board-search"
                    type="search"
                    hidden={move || view.get() != WorkspaceView::Board}
                    placeholder={move || i18n.text("message.title_ticket_or_description")}
                    aria-label={move || i18n.text("message.search_tasks")}
                    prop:value={move || search.get()}
                    on:input={move |event| search.set(event_target_value(&event))}
                />
                <crate::button::Button
                    {..}
                    id="configure-columns"
                    hidden={move || view.get() != WorkspaceView::Board}
                    type="button"
                    on:click={move |_| show_kanban_settings.set(true)}
                >
                    {move || i18n.text("message.configure_kanban_columns")}
                </crate::button::Button>
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get())}
                    {..}
                    id="sync-button"
                    type="button"
                    hidden={move || {
                        board.settings.get().string_equals("ticketProvider", "local").unwrap_or(true)
                    }}
                    on:click={move |_| {
                        if pending.get_untracked() {
                            return;
                        }
                        let Ok(credential) = token.get_value() else {
                            return;
                        };
                        ticket_sync_notice.set("message.ticket_sync_running".into());
                        let provider = field(&board.settings.get_untracked(), "ticketProvider");
                        let project = projects
                            .with_untracked(|items| {
                                items
                                    .iter()
                                    .find(|project| field(project, "repo") == repository.get_untracked())
                                    .cloned()
                            });
                        pending.set(true);
                        error.set(String::new());
                        spawn_local(async move {
                            let result = crate::ticket_sync::synchronize(&credential, &provider, project)
                                .await;
                            let _ = board.refresh(&credential).await;
                            pending.set(false);
                            ticket_sync_notice.set(String::new());
                            match result {
                                Ok(crate::ticket_sync::Outcome::ConfigurationRequired) => {
                                    settings_section.set("tickets");
                                    navigate.run(WorkspaceView::Settings);
                                }
                                Ok(crate::ticket_sync::Outcome::Complete) => {
                                    ticket_sync_notice.set("message.ticket_sync_complete".into())
                                }
                                Err(message) => error.set(message),
                            }
                        });
                    }}
                >
                    {move || i18n.text("message.synchronize_tickets")}
                </crate::button::Button>
            </section>
            <p
                id="credential-status"
                role="status"
                class="connection-notice"
                hidden={move || {
                    !board.settings.with(|settings| settings.has_text("credentialError").unwrap_or(false))
                }}
            >
                {move || board.settings.with(|settings| i18n.text(&field(settings, "credentialError")))}
            </p>
            <p
                class="connection-notice"
                id="live-status"
                role="status"
                hidden={move || {
                    !workspace_controls() || token.get_value().is_err()
                        || (board.connected.get() && board.sync_error.get().is_empty())
                }}
                data-connected={move || board.connected.get().to_string()}
            >
                {move || {
                    i18n.text(
                        &(if !board.sync_error.get().is_empty() {
                            board.sync_error.get()
                        } else {
                            "message.updates_interrupted_reconnecting".into()
                        }),
                    )
                }}
            </p>
            <div
                id="focus-bar"
                class="focus-bar"
                hidden={move || view.get() != WorkspaceView::Board || cards.get().is_empty()}
            >
                <span class="results-count">
                    <strong id="total">
                        {move || {
                            visible(false)
                                .into_iter()
                                .filter(|card| matches_focus(
                                    card,
                                    &cards.get(),
                                    &board.runs.get(),
                                    &focus_filter.get(),
                                ))
                                .count()
                        }}
                    </strong>
                    " "
                    {move || i18n.text("message.task_s")}
                </span>
                <div class="focus-options">
                    {[
                        ("all", "message.all"),
                        ("review", "message.to_review"),
                        ("attention", "message.to_resume"),
                    ]
                        .into_iter()
                        .map(move |(filter, label)| {
                            view! {
                                <crate::button::Button
                                    {..}
                                    data-focus={filter}
                                    aria-pressed={move || (focus_filter.get() == filter).to_string()}
                                    on:click={move |_| focus_filter.set(filter.into())}
                                >
                                    {move || i18n.text(label)}
                                    <span hidden={filter
                                        == "all"}>
                                        {move || {
                                            visible(false)
                                                .iter()
                                                .filter(|card| matches_focus(
                                                    card,
                                                    &cards.get(),
                                                    &board.runs.get(),
                                                    filter,
                                                ))
                                                .count()
                                        }}
                                    </span>
                                </crate::button::Button>
                            }
                        })
                        .collect_view()}
                </div>
                <span class="keyboard-hint">
                    <kbd>"/"</kbd>
                    " "
                    {move || i18n.text("message.search_2fe057eb")}
                    " · "
                    <kbd>"N"</kbd>
                    " "
                    {move || i18n.text("message.new_task_b7d039ad")}
                </span>
            </div>
            <crate::time::TimerBanner board token on_task={open_task} />
            <p id="ticket-sync-notice" role="status" hidden={move || ticket_sync_notice.get().is_empty()}>
                {move || i18n.text(&ticket_sync_notice.get())}
            </p>
            <p role="alert" id="error">
                {move || i18n.text(&error.get())}
            </p>
            <p class="loading" role="status" hidden={move || !loading.get()}>
                {move || i18n.text("message.loading_the_board")}
            </p>
            <section
                id="welcome"
                class="welcome-guide"
                aria-labelledby="welcome-title"
                hidden={move || !onboarding()}
            >
                <div class="welcome-intro">
                    <h2 id="welcome-title">{move || i18n.text("message.prepare_your_first_task")}</h2>
                    <p>{move || i18n.text("message.welcome_task_intro")}</p>
                </div>
                <div class="welcome-paths">
                    <div class="welcome-path">
                        <h3>{move || i18n.text("message.find_your_code")}</h3>
                        <p>{move || i18n.text("message.save_a_project_folder")}</p>
                        <crate::button::Button
                            {..}
                            id="welcome-projects"
                            type="button"
                            on:click={move |_| navigate.run(WorkspaceView::Projects)}
                        >
                            {move || i18n.text("message.add_a_project")}
                        </crate::button::Button>
                    </div>
                    <div class="welcome-path">
                        <h3>{move || i18n.text("message.describe_the_work")}</h3>
                        <p>{move || i18n.text("message.first_task_help")}</p>
                        <crate::button::Button
                            variant={crate::button::ButtonVariant::Primary}
                            class="primary"
                            {..}
                            id="welcome-create"
                            type="button"
                            on:click={move |_| open_new.run(())}
                        >
                            {move || i18n.text("message.new_task")}
                        </crate::button::Button>
                    </div>
                </div>
                <div class="welcome-settings">
                    <p>{move || i18n.text("message.connect_tools_later")}</p>
                    <crate::button::Button
                        variant={crate::button::ButtonVariant::Ghost}
                        class="text-button"
                        {..}
                        id="welcome-settings"
                        type="button"
                        on:click={move |_| navigate.run(WorkspaceView::Settings)}
                    >
                        {move || i18n.text("message.open_settings")}
                    </crate::button::Button>
                </div>
            </section>
            <section
                aria-label={move || i18n.text("message.task_board")}
                id="prototype-board"
                class="board"
                tabindex="0"
                hidden={move || view.get() != WorkspaceView::Board || visible(false).is_empty()}
                aria-busy={move || loading.get().to_string()}
            >
                <For
                    each={move || {
                        board
                            .projects
                            .get()
                            .iter()
                            .find(|project| field(project, "repo") == repository.get())
                            .and_then(|project| atelier_core::application::kanban::for_project(project).ok())
                            .unwrap_or_default()
                            .columns
                    }}
                    key={|column| column.status.clone()}
                    children={move |entry| {
                        let status = STATUSES
                            .iter()
                            .find(|(status, _)| *status == entry.status)
                            .map(|(status, _)| *status)
                            .unwrap_or("todo");
                        let label = move || {
                            board
                                .projects
                                .with(|projects| crate::kanban_settings::project_column_name(
                                    i18n,
                                    projects,
                                    &repository.get(),
                                    status,
                                ))
                        };
                        let column = move || {
                            if view.get() == WorkspaceView::Archives {
                                Vec::new()
                            } else {
                                visible(false)
                                    .into_iter()
                                    .filter(|card| {
                                        field(&card.fields, "status") == status
                                            && matches_focus(
                                                card,
                                                &cards.get(),
                                                &board.runs.get(),
                                                &focus_filter.get(),
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            }
                        };
                        view! {
                            <section
                                class={format!("column {status}")}
                                data-status={status}
                                aria-label={label}
                                on:dragover={move |event: leptos::ev::DragEvent| {
                                    if dragging.get_untracked().is_some() && !pending.get_untracked() {
                                        event.prevent_default();
                                        if let Some(transfer) = event.data_transfer() {
                                            transfer.set_drop_effect("move");
                                        }
                                    }
                                }}
                                on:drop={move |event: leptos::ev::DragEvent| {
                                    if dragging.get_untracked().is_some() {
                                        event.prevent_default();
                                        move_card.run(status);
                                    }
                                }}
                            >
                                <div class="column-heading">
                                    <span class="status-dot"></span>
                                    {label}
                                    <span class="count">{move || column().len()}</span>
                                </div>
                                <For
                                    each={column}
                                    key={|card| card.id.clone()}
                                    children={move |card| {
                                        let id = StoredValue::new(card.id);
                                        let current = move || {
                                            cards
                                                .with(|cards| {
                                                    cards.iter().find(|card| card.id == id.get_value()).cloned()
                                                })
                                        };
                                        let running = move || {
                                            board
                                                .runs
                                                .with(|runs| {
                                                    runs.iter()
                                                        .any(|run| {
                                                            field(run, "cardId") == id.get_value()
                                                                && matches!(
                                                                    field(run, "status").as_str(),
                                                                    "starting" | "running" | "stopping"
                                                                )
                                                        })
                                                })
                                        };
                                        let busy = move || {
                                            pending.get()
                                                || current()
                                                    .is_some_and(|card| {
                                                        card.fields.flag("publishing").unwrap_or(false)
                                                            || card
                                                                .fields
                                                                .get::<atelier_core::document::Document>("automation")
                                                                .ok()
                                                                .flatten()
                                                                .is_some_and(|flow| field(&flow, "state") == "running")
                                                    })
                                        };
                                        view! {
                                            <crate::card::Card
                                                class="card"
                                                {..}

                                                data-id={id.get_value()}
                                                aria-labelledby={format!("card-open-{}", id.get_value())}
                                                draggable={move || (!busy()).to_string()}
                                                class:dragging={move || {
                                                    dragging
                                                        .with(|value| {
                                                            value
                                                                .as_ref()
                                                                .is_some_and(|(card, _)| card == &id.get_value())
                                                        })
                                                }}
                                                on:dragstart={move |event: leptos::ev::DragEvent| {
                                                    if busy() {
                                                        event.prevent_default();
                                                        return;
                                                    }
                                                    if let Some(card) = current() {
                                                        if let Some(transfer) = event.data_transfer() {
                                                            if transfer.set_data("text/plain", &card.id).is_err() {
                                                                event.prevent_default();
                                                                return;
                                                            }
                                                            transfer.set_effect_allowed("move");
                                                        }
                                                        dragging
                                                            .set(Some((card.id, field(&card.fields, "status"))));
                                                    }
                                                }}
                                                on:dragend={move |_| dragging.set(None)}
                                            >
                                                <crate::card::CardHeader>
                                                    <span class="card-label">
                                                        {move || {
                                                            current()
                                                                .and_then(|card| {
                                                                    card.fields
                                                                        .get::<atelier_core::document::Document>("ticket")
                                                                        .ok()
                                                                        .flatten()
                                                                })
                                                                .map(|ticket| field(&ticket, "key"))
                                                                .filter(|key| !key.is_empty())
                                                                .unwrap_or_else(|| i18n.text("message.local_task"))
                                                        }}
                                                    </span>
                                                    <div class="card-actions">
                                                        <crate::button::Button
                                                            class="card-actions-toggle"
                                                            {..}
                                                            type="button"
                                                            id={format!("card-actions-toggle-{}", id.get_value())}
                                                            popovertarget={format!("card-actions-{}", id.get_value())}
                                                            aria-label={move || i18n.text("message.task_actions")}
                                                            title={move || i18n.text("message.task_actions")}
                                                        >
                                                            <svg
                                                                width="16"
                                                                height="16"
                                                                viewBox="0 0 16 16"
                                                                fill="currentColor"
                                                                aria-hidden="true"
                                                            >
                                                                <circle cx="3" cy="8" r="1.5" />
                                                                <circle cx="8" cy="8" r="1.5" />
                                                                <circle cx="13" cy="8" r="1.5" />
                                                            </svg>
                                                        </crate::button::Button>
                                                        <div
                                                            id={format!("card-actions-{}", id.get_value())}
                                                            class="card-action-menu"
                                                            popover="auto"
                                                        >
                                                            <crate::button::Button
                                                                variant={crate::button::ButtonVariant::Danger}
                                                                disabled={Signal::derive(move || pending.get())}
                                                                class="danger"
                                                                {..}
                                                                type="button"
                                                                hidden={move || !running()}
                                                                popovertarget={format!("card-actions-{}", id.get_value())}
                                                                popovertargetaction="hide"
                                                                on:click={move |_| {
                                                                    card_action.run((id.get_value(), "stop"))
                                                                }}
                                                            >
                                                                <svg
                                                                    width="16"
                                                                    height="16"
                                                                    viewBox="0 0 24 24"
                                                                    fill="none"
                                                                    stroke="currentColor"
                                                                    stroke-width="1.7"
                                                                    aria-hidden="true"
                                                                >
                                                                    <rect x="5" y="5" width="14" height="14" rx="2" />
                                                                </svg>
                                                                {move || i18n.text("message.stop_the_agent")}
                                                            </crate::button::Button>
                                                            <crate::button::Button
                                                                disabled={busy}
                                                                {..}
                                                                type="button"
                                                                hidden={running}
                                                                popovertarget={format!("card-actions-{}", id.get_value())}
                                                                popovertargetaction="hide"
                                                                on:click={move |_| {
                                                                    card_action.run((id.get_value(), "archive"))
                                                                }}
                                                            >
                                                                <NavigationIcon kind="archives" view_icon=true />
                                                                {move || i18n.text("message.archive_8614cf00")}
                                                            </crate::button::Button>
                                                            <crate::button::Button
                                                                variant={crate::button::ButtonVariant::Danger}
                                                                disabled={busy}
                                                                class="danger"
                                                                {..}
                                                                type="button"
                                                                hidden={running}
                                                                popovertarget={format!("card-actions-{}", id.get_value())}
                                                                popovertargetaction="hide"
                                                                on:click={move |_| {
                                                                    card_action.run((id.get_value(), "delete"))
                                                                }}
                                                            >
                                                                <svg
                                                                    width="16"
                                                                    height="16"
                                                                    viewBox="0 0 24 24"
                                                                    fill="none"
                                                                    stroke="currentColor"
                                                                    stroke-width="1.7"
                                                                    aria-hidden="true"
                                                                >
                                                                    <path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7" />
                                                                </svg>
                                                                {move || i18n.text("message.delete_locally")}
                                                            </crate::button::Button>
                                                        </div>
                                                    </div>
                                                </crate::card::CardHeader>
                                                <crate::button::Button
                                                    class="card-title card-open"
                                                    {..}
                                                    type="button"
                                                    id={format!("card-open-{}", id.get_value())}
                                                    on:click={move |_| open_task.run(id.get_value())}
                                                >
                                                    {move || {
                                                        current().map(|card| card.title.clone()).unwrap_or_default()
                                                    }}
                                                </crate::button::Button>
                                                <span class="card-description">
                                                    {move || {
                                                        current()
                                                            .map(|card| field(&card.fields, "description"))
                                                            .unwrap_or_default()
                                                    }}
                                                </span>
                                                {move || {
                                                    current()
                                                        .and_then(|card| {
                                                            card.fields
                                                                .get::<
                                                                    Vec<atelier_core::domain::checklist::Criterion>,
                                                                >("checklist")
                                                                .ok()
                                                                .flatten()
                                                        })
                                                        .filter(|criteria| !criteria.is_empty())
                                                        .map(|criteria| {
                                                            view! {
                                                                <span class="card-criteria">
                                                                    {i18n
                                                                        .text("message.0_1_criteria_checked")
                                                                        .replace(
                                                                            "{0}",
                                                                            &criteria
                                                                                .iter()
                                                                                .filter(|criterion| criterion.done)
                                                                                .count()
                                                                                .to_string(),
                                                                        )
                                                                        .replace("{1}", &criteria.len().to_string())}
                                                                </span>
                                                            }
                                                        })
                                                }}
                                                <crate::card::CardFooter class="card-bottom">
                                                    {move || {
                                                        current()
                                                            .and_then(|card| {
                                                                let agent = field(&card.fields, "agent");
                                                                board
                                                                    .settings
                                                                    .with(|settings| {
                                                                        atelier_core::domain::agent_options::providers(settings)
                                                                            .unwrap_or_default()
                                                                            .into_iter()
                                                                            .find(|profile| field(profile, "id") == agent)
                                                                    })
                                                            })
                                                            .map(|profile| {
                                                                view! {
                                                                    <span class="agent-pill">{field(&profile, "name")}</span>
                                                                }
                                                            })
                                                    }}
                                                    <span class="project-pill">
                                                        {move || {
                                                            current()
                                                                .map(|card| {
                                                                    let repo = field(&card.fields, "repo");
                                                                    projects
                                                                        .with(|projects| {
                                                                            projects
                                                                                .iter()
                                                                                .find(|project| field(project, "repo") == repo)
                                                                                .map(|project| field(project, "name"))
                                                                        })
                                                                        .unwrap_or_else(|| {
                                                                            if repo.is_empty() {
                                                                                i18n.text("message.no_project")
                                                                            } else {
                                                                                repo.rsplit(['/', '\\'])
                                                                                    .find(|part| !part.is_empty())
                                                                                    .unwrap_or(&repo)
                                                                                    .to_owned()
                                                                            }
                                                                        })
                                                                })
                                                                .unwrap_or_default()
                                                        }}
                                                    </span>
                                                    <span
                                                        class="card-live"
                                                        hidden={move || {
                                                            !board
                                                                .runs
                                                                .with(|runs| {
                                                                    runs.iter()
                                                                        .any(|run| {
                                                                            field(run, "cardId") == id.get_value()
                                                                                && field(run, "status") == "running"
                                                                        })
                                                                })
                                                        }}
                                                    >
                                                        {move || i18n.text("message.work_in_progress")}
                                                    </span>
                                                </crate::card::CardFooter>
                                            </crate::card::Card>
                                        }
                                    }}
                                />
                                <p class="empty" hidden={move || !column().is_empty() || loading.get()}>
                                    {move || i18n.text("message.nothing_yet")}
                                </p>
                                <crate::button::Button
                                    class="column-add"
                                    {..}
                                    type="button"
                                    hidden={status != "todo"}
                                    aria-label={move || i18n.text("message.add_a_task")}
                                    on:click={move |_| open_new.run(())}
                                >
                                    "＋"
                                </crate::button::Button>
                            </section>
                        }
                    }}
                />
            </section>
            <Show when={move || {
                matches!(
                    view.get(),
                    WorkspaceView::Overview
                    | WorkspaceView::Agents
                    | WorkspaceView::Processes
                    | WorkspaceView::Worktrees
                )
            }}>
                <workspace_views::WorkspaceViews
                    board
                    token
                    view={Signal::derive(move || view.get())}
                    project={Signal::derive(move || repository.get())}
                    on_open={open_task}
                    on_view={navigate}
                    on_focus={Callback::new(move |filter: String| {
                        focus_filter.set(filter);
                        search.set(String::new());
                        navigate.run(WorkspaceView::Board);
                    })}
                    on_projects={Callback::new(move |()| navigate.run(WorkspaceView::Projects))}
                />
            </Show>
            <Show when={move || view.get() == WorkspaceView::Archives}>
                <section id="workspace-archives" class="workspace-panel">
                    <div class="panel-heading">
                        <div>
                            <h2>{move || i18n.text("message.your_history")}</h2>
                            <p class="hint">
                                {move || {
                                    i18n.text(
                                        "message.archived_tasks_with_their_time_entries_branches_and_logs_restori",
                                    )
                                }}
                            </p>
                        </div>
                    </div>
                    <For
                        each={move || visible(false)}
                        key={|card| card.id.clone()}
                        children={move |card| {
                            let id = StoredValue::new(card.id.clone());
                            view! {
                                <crate::card::Card class="workspace-row archive-row" {..}  data-id={id.get_value()}>
                                    <div>
                                        <span class="section-kicker">
                                            {move || i18n.text("message.archived_task")}
                                        </span>
                                        <h3>{card.title.clone()}</h3>
                                        <p class="hint">{move || i18n.text("message.history_kept")}</p>
                                    </div>
                                    <div class="row-actions">
                                        <crate::button::Button
                                            class="card-open"
                                            {..}
                                            id={format!("card-open-{}", id.get_value())}
                                            on:click={move |_| open_task.run(id.get_value())}
                                        >
                                            {move || i18n.text("message.open_task")}
                                        </crate::button::Button>
                                        <crate::button::Button
                                            disabled={Signal::derive(move || pending.get())}
                                            {..}
                                            on:click={move |_| {
                                                if pending.get_untracked() {
                                                    return;
                                                }
                                                let Ok(token) = token.get_value() else {
                                                    return;
                                                };
                                                pending.set(true);
                                                spawn_local(async move {
                                                    let result = write(
                                                            &token,
                                                            Method::POST,
                                                            &format!("/api/cards/{}/archive", id.get_value()),
                                                            &serde_json::json!(
                                                                {
                                "archived":false
                            }
                                                            ),
                                                        )
                                                        .await;
                                                    match result {
                                                        Ok(_) => {
                                                            let _ = board.refresh(&token).await;
                                                        }
                                                        Err(message) => error.set(message),
                                                    }
                                                    pending.set(false);
                                                });
                                            }}
                                        >
                                            {move || i18n.text("message.restore")}
                                        </crate::button::Button>
                                    </div>
                                </crate::card::Card>
                            }
                        }}
                    />
                    <p class="empty" hidden={move || !visible(false).is_empty()}>
                        {move || i18n.text("message.no_archived_tasks")}
                    </p>
                </section>
            </Show>
            <Show when={move || view.get() == WorkspaceView::Brainstorm}>
                <BrainstormPage board token on_open={open_task} on_new={open_new} />
            </Show>
            {move || {
                selected
                    .get()
                    .filter(|_| view.get() == WorkspaceView::Task)
                    .map(|card| {
                        view! {
                            <TaskEditor
                                card
                                board
                                token
                                on_open={open_task}
                                on_close={Callback::new(move |()| {
                                    commit_view.run(return_view.get_untracked())
                                })}
                            />
                        }
                    })
            }}
            {move || match view.get() {
                WorkspaceView::Settings | WorkspaceView::Updates => {
                    Some(
                        view! {
                            <Settings
                                board
                                token
                                initial_tab={if view.get() == WorkspaceView::Updates {
                                    "updates"
                                } else {
                                    settings_section.get_untracked()
                                }}
                                on_close={Callback::new(move |()| commit_view.run(WorkspaceView::Overview))}
                            />
                        },
                    )
                }
                _ => None,
            }}
            <Show when={move || view.get() == WorkspaceView::Time}>
                <crate::time_report::TimeReport
                    board
                    token
                    loading={Memo::new(move |_| loading.get())}
                    load_error={Signal::derive(move || {
                        if board.loaded.get() { String::new() } else { error.get() }
                    })}
                    on_close={Callback::new(move |()| commit_view.run(WorkspaceView::Overview))}
                    on_task={open_task}
                />
            </Show>
            <Show when={move || view.get() == WorkspaceView::Projects}>
                <Projects
                    board
                    repository
                    token
                    on_close={Callback::new(move |()| commit_view.run(WorkspaceView::Overview))}
                />
            </Show>
            <footer>
                "ATELIER "
                <span>{move || i18n.text("message.a_little_less_friction_a_little_more_done")}</span>
                <span>{move || i18n.text("message.local_storage_sqlite")}</span>
            </footer>
            <Show when={move || show_kanban_settings.get()}>
                <crate::kanban_settings::KanbanSettings
                    board
                    token
                    repository
                    on_close={Callback::new(move |()| show_kanban_settings.set(false))}
                />
            </Show>
            <Show when={move || show_agents.get()}>
                <AgentProfiles board token on_close={Callback::new(move |()| show_agents.set(false))} />
            </Show>
            <Show when={move || show_backups.get()}>
                <Backups board token on_close={Callback::new(move |()| show_backups.set(false))} />
            </Show>
            {move || {
                deleting_card
                    .get()
                    .map(|card| {
                        let id = card.id.clone();
                        view! {
                            <DeleteCardDialog
                                card
                                pending
                                error
                                on_confirm={Callback::new(move |()| {
                                    card_action.run((id.clone(), "confirm-delete"))
                                })}
                                on_close={Callback::new(move |()| deleting_card.set(None))}
                            />
                        }
                    })
            }}
            <Show when={move || show_diagnostics.get()}>
                <DiagnosticsDialog
                    board
                    token
                    on_close={Callback::new(move |()| show_diagnostics.set(false))}
                />
            </Show>
            <Show when={move || command_open.get()}>
                <CommandPalette
                    board
                    on_close={Callback::new(move |()| command_open.set(false))}
                    on_open={open_task}
                    on_new={open_new}
                    on_project={Callback::new(move |repo: String| {
                        repository.set(repo);
                        search.set(String::new());
                        focus_filter.set("all".into());
                        navigate.run(WorkspaceView::Overview);
                    })}
                    on_projects={Callback::new(move |()| navigate.run(WorkspaceView::Projects))}
                    on_diagnostics={Callback::new(move |()| show_diagnostics.set(true))}
                />
            </Show>
        </main>
        <new_task::NewTask
            board
            token
            repository
            loading
            pending
            open_request={new_task_request}
            on_created={Callback::new(move |(id, repo): (String, String)| {
                focus_filter.set("all".into());
                search.set(String::new());
                repository.set(repo);
                view.set(WorkspaceView::Board);
                open_task.run(id);
            })}
            on_close={Callback::new(move |()| focus_element(
                if onboarding() { "welcome-create" } else { "create-button" },
            ))}
        />
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
        "brainstorm"=>view! { <path d="M9 18h6m-6 3h6M8 14a7 7 0 1 1 8 0l-1 2H9z" /> }.into_any(),
        "collapse"=>view! { <path d="m14 6-6 6 6 6" /> }.into_any(),
        "menu"=>view! { <path d="M4 6h16M4 12h16M4 18h16" /> }.into_any(),
        "overview"=>view! {
            <rect x="3" y="3" width="7" height="7" rx="1.5" />
            <rect x="14" y="3" width="7" height="7" rx="1.5" />
            <rect x="3" y="14" width="7" height="7" rx="1.5" />
            <rect x="14" y="14" width="7" height="7" rx="1.5" />
        }.into_any(),
        "projects"=>view! {
            <path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
            <path d="M3 10h18" />
        }.into_any(),
        "settings"=>view! {
            <path d="M4 7h16M4 17h16" />
            <circle cx="9" cy="7" r="3" />
            <circle cx="15" cy="17" r="3" />
        }.into_any(),
        "diagnostics"=>view! { <path d="M3 12h4l3-7 4 14 3-7h4" /> }.into_any(),
        "updates"=>view! {
            <path d="M20 7v5h-5M4 17v-5h5" />
            <path d="M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1" />
        }.into_any(),
        "backups"=>view! {
            <path d="M4 3h13l3 3v15H4Z" />
            <path d="M8 3v6h8V3M8 21v-8h8v8" />
        }.into_any(),
        "board"=>view! {
            <rect x="3" y="4" width="18" height="16" rx="2" />
            <path d="M9 4v16M15 4v16" />
        }.into_any(),
        "agents"=>view! { <path d="M8 3v3m8-3v3M6 6h12v14H6zM3 10h3m12 0h3M3 16h3m12 0h3M9 10h6M9 15h6" /> }.into_any(),
        "processes"=>view! { <path d="m9 5 11 7-11 7z" /> }.into_any(),
        "worktrees"=>view! {
            <circle cx="6" cy="5" r="2" />
            <circle cx="6" cy="19" r="2" />
            <circle cx="18" cy="5" r="2" />
            <path d="M6 7v10m12-10v3a5 5 0 0 1-5 5H6" />
        }.into_any(),
        "time"=>view! {
            <circle cx="12" cy="12" r="9" />
            <path d="M12 7v5l3 2" />
        }.into_any(),
        _=>view! { <path d="M4 8h16v12H4zM3 4h18v4H3zM9 12h6" /> }.into_any(),
    };
    view! {
        <svg
            class={if view_icon { "view-icon" } else { "nav-icon" }}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width={if view_icon { "1.6" } else { "1.7" }}
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            focusable="false"
        >
            {shape}
        </svg>
    }
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
    let results = Memo::new(move |_| {
        let mut items = vec![
            (
                CommandTarget::New,
                i18n.text("message.create_a_task"),
                i18n.text("message.new_local_task"),
            ),
            (
                CommandTarget::Projects,
                i18n.text("message.manage_projects"),
                i18n.text("message.local_directories"),
            ),
            (
                CommandTarget::Diagnostics,
                i18n.text("message.test_connections"),
                i18n.text("message.git_agents_and_services"),
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
                .unwrap_or_else(|| i18n.text("message.local_task_71560e02"));
            (CommandTarget::Card(card.id), card.title.clone(), ticket)
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
    view! {
        <modal::Modal
            id="command-dialog"
            labelledby="command-title"
            node_ref={dialog}
            on_close={Callback::new(move |()| close())}
        >
            <modal::ModalHeader class="command-heading" title_id="command-title" title=Signal::derive(move||i18n.text("message.quick_search")) on_close=Callback::new(move|()|close())/>
            <div class="command-search">
                <crate::form::Field
                    id="command-query"
                    label={Signal::derive(move || i18n.text("message.project_task_or_action"))}
                >
                    <crate::form::Input
                        {..}
                        id="command-query"
                        type="search"
                        autofocus=true
                        autocomplete="off"
                        aria-controls="command-results"
                        placeholder={move || i18n.text("message.a_ticket_a_project_an_action")}
                        prop:value={move || query.get()}
                        on:input={move |event| {
                            query.set(event_target_value(&event));
                            index.set(0);
                        }}
                        on:keydown={move |event: leptos::ev::KeyboardEvent| {
                            match event.key().as_str() {
                                "ArrowDown" => {
                                    event.prevent_default();
                                    index
                                        .update(|index| {
                                            *index = (*index + 1)
                                                .min(results.with(Vec::len).saturating_sub(1));
                                        });
                                }
                                "ArrowUp" => {
                                    event.prevent_default();
                                    index.update(|index| *index = index.saturating_sub(1));
                                }
                                "Enter" => {
                                    event.prevent_default();
                                    execute.run(index.get_untracked());
                                }
                                _ => {}
                            }
                        }}
                    />
                </crate::form::Field>
            </div>
            <div id="command-results" aria-live="polite">
                {move || {
                    results
                        .get()
                        .into_iter()
                        .enumerate()
                        .map(|(position, (target, title, detail))| {
                            view! {
                                <crate::button::Button
                                    class="command-result"
                                    {..}
                                    type="button"
                                    aria-current={move || (index.get() == position).to_string()}
                                    on:click={move |_| execute.run(position)}
                                >
                                    <span class="command-copy">
                                        <strong>{title}</strong>
                                        <small>{detail}</small>
                                    </span>
                                    <span class="command-kind">
                                        {i18n
                                            .text(
                                                match target {
                                                    CommandTarget::Project(_) => "message.project",
                                                    CommandTarget::Card(_) => "message.task",
                                                    _ => "message.command_action",
                                                },
                                            )}
                                    </span>
                                    <span class="command-return" aria-hidden="true">
                                        "↵"
                                    </span>
                                </crate::button::Button>
                            }
                        })
                        .collect_view()
                }}
                <p class="empty" hidden={move || !results.with(Vec::is_empty)}>
                    {move || i18n.text("message.no_results_try_a_title_jira_id_or_project_name")}
                </p>
            </div>
            <div class="search-help">
                <span>
                    <kbd>"↑"</kbd>
                    <kbd>"↓"</kbd>
                    {move || i18n.text("message.browse_622500d8")}
                </span>
                <span>
                    <kbd>{move || i18n.text("message.enter")}</kbd>
                    {move || i18n.text("message.open")}
                </span>
                <span>
                    <kbd>{move || i18n.text("message.esc")}</kbd>
                    {move || i18n.text("message.close_d932b8d4")}
                </span>
            </div>
        </modal::Modal>
    }
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
    let close = move || {
        if !pending.get_untracked() {
            if let Some(dialog) = dialog.get_untracked() {
                dialog.close();
            }
            on_close.run(());
            focus_element("diagnostics-button");
        }
    };
    view! {
        <crate::modal::Modal
            id="diagnostics-dialog"
            node_ref={dialog}
            labelledby="diagnostics-title"
            on_close={Callback::new(move |()| close())}
        >
            <crate::modal::ModalHeader class="inset" title_id="diagnostics-title" title=Signal::derive(move||i18n.text("message.service_status")) busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|close())/>
            <p class="hint">
                {move || {
                    i18n.text("message.explicit_check_of_the_saved_profile_cli_version_and_login_status")
                }}
            </p>
            <crate::form::Field label=Signal::derive(move || i18n.text("message.agent"))><crate::form::Select
                    {..}
                    disabled={move || pending.get()}
                    on:change={move |event| {
                        let id = event_target_value(&event);
                        profile
                            .set(
                                atelier_core::domain::agent_options::providers(
                                        &board.settings.get_untracked(),
                                    )
                                    .unwrap_or_default()
                                    .into_iter()
                                    .find(|item| field(item, "id") == id),
                            );
                    }}
                >
                    <option value="">{move || i18n.text("message.choose_a_profile")}</option>
                    <For
                        each={move || {
                            atelier_core::domain::agent_options::providers(&board.settings.get())
                                .unwrap_or_default()
                        }}
                        key={|profile| field(profile, "id")}
                        children={move |profile| {
                            view! { <option value={field(&profile, "id")}>{field(&profile, "name")}</option> }
                        }}
                    />
                </crate::form::Select></crate::form::Field>
            <div id="diagnostics-results" aria-live="polite">
                <agent_diagnostics::AgentDiagnostics profile token pending blocked={Memo::new(|_| false)} />
            </div>
        </crate::modal::Modal>
    }
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
    view! {
        <crate::modal::Modal
            id="confirm-dialog"
            class="confirm-dialog"
            labelledby="confirm-title"
            describedby="confirm-message"
            node_ref={dialog}
            busy={Signal::derive(move || pending.get())}
            on_close={Callback::new(move |()| close())}
        >
            <crate::form::Form busy=Signal::derive(move||pending.get()) on_submit=Callback::new(move |event: leptos::ev::SubmitEvent| {
                event.prevent_default();
                on_confirm.run(());
            }) {..}  >
                <crate::modal::ModalHeader class="inset" title_id="confirm-title" title=Signal::derive(move||i18n.text("message.permanently_delete_this_task")) busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|close())/>
                <p>{card.title.clone()}</p>
                <p id="confirm-message">
                    {move || i18n.text("message.permanently_delete_this_task_this_cannot_be_undone")}
                </p>
                <p class="error" role="alert">
                    {move || i18n.text(&error.get())}
                </p>
                <crate::modal::ModalActions {..} >
                    <crate::button::Button
                        disabled={Signal::derive(move || pending.get())}
                        {..}
                        type="button"
                        autofocus=true
                        on:click={move |_| close()}
                    >
                        {move || i18n.text("message.cancel")}
                    </crate::button::Button>
                    <crate::button::Button
                        disabled={Signal::derive(move || pending.get())}
                        class="confirm-action"
                        {..}
                        id="confirm-action"
                        type="submit"
                    >
                        {move || i18n.text("message.delete_permanently")}
                    </crate::button::Button>
                </crate::modal::ModalActions>
            </crate::form::Form>
        </crate::modal::Modal>
    }
}
fn field(document: &atelier_core::document::Document, key: &str) -> String {
    document
        .get::<String>(key)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn main() {
    leptos::mount::mount_to_body(App);
}
