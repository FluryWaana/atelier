use crate::{NavigationGuard, WorkspaceView, field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    contracts::{Card, ServiceSession, ServiceState, ServiceStatus, Worktree},
    document::Document,
    domain::{agent_options::providers, projects::same_repository, time::duration_text},
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

fn title(card: &Card) -> String {
    card.title.clone()
}
fn nested(document: &Document, key: &str) -> Document {
    document.get(key).ok().flatten().unwrap_or_default()
}
fn active(run: &Document) -> bool {
    matches!(
        field(run, "status").as_str(),
        "starting" | "running" | "stopping"
    )
}
fn needs_attention(card: &Card, runs: &[Document], cards: &[Card]) -> bool {
    if field(&card.fields, "status") == "done" {
        return false;
    }
    let blockers = serde_json::to_value(card).ok().is_some_and(|card| {
        let cards = cards
            .iter()
            .filter_map(|card| serde_json::to_value(card).ok())
            .collect::<Vec<_>>();
        !atelier_core::domain::tasks::blockers(&card, &cards).is_empty()
    });
    blockers
        || field(&nested(&card.fields, "ticketSync"), "state") == "error"
        || matches!(
            field(&nested(&card.fields, "automation"), "state").as_str(),
            "error" | "interrupted" | "cancelled" | "blocked"
        )
        || runs
            .iter()
            .find(|run| field(run, "cardId") == card.id)
            .is_some_and(|run| {
                !field(run, "attention").is_empty()
                    || run
                        .get::<Vec<serde_json::Value>>("interactions")
                        .ok()
                        .flatten()
                        .is_some_and(|items| !items.is_empty())
                    || matches!(
                        field(run, "status").as_str(),
                        "failed" | "interrupted" | "cancelled"
                    )
            })
}
fn agent_name(settings: &Document, run: &Document) -> String {
    let id = field(run, "agent");
    providers(settings)
        .unwrap_or_default()
        .iter()
        .find(|profile| field(profile, "id") == id)
        .map(|profile| field(profile, "name"))
        .filter(|name| !name.is_empty())
        .unwrap_or(id)
}
fn agent_state(run: &Document, now: f64) -> &'static str {
    if active(run)
        && run
            .get::<Vec<serde_json::Value>>("interactions")
            .ok()
            .flatten()
            .is_some_and(|items| !items.is_empty())
    {
        return "message.waiting_for_your_reply";
    }
    if !field(run, "attention").is_empty() {
        return if active(run) {
            "message.attention_needed"
        } else {
            "message.blocked_check_the_log"
        };
    }
    match field(run, "status").as_str() {
        "starting" => "message.starting",
        "stopping" => "message.stopping_1cbeb899",
        "completed" => "message.completed_13ecee2c",
        "cancelled" => "message.stopped_c1d5d88b",
        "interrupted" => "message.interrupted",
        "failed" => "message.failed",
        "running" => {
            let event = field(run, "lastEventAt");
            let started = if event.is_empty() {
                field(run, "startedAt")
            } else {
                event
            };
            if now - js_sys::Date::parse(&started) > 60_000.0 {
                "message.no_signal_check_status"
            } else {
                "message.working_dcaf5e34"
            }
        }
        _ => "message.needs_review",
    }
}
fn elapsed(run: &Document, now: f64) -> String {
    let start = js_sys::Date::parse(&field(run, "startedAt"));
    let end = if active(run) {
        now
    } else {
        js_sys::Date::parse(&field(run, "finishedAt"))
    };
    if start.is_finite() && end.is_finite() {
        duration_text(((end - start).max(0.0) / 1000.0) as u64)
    } else {
        String::new()
    }
}
fn service_active(service: &ServiceState) -> bool {
    matches!(
        service.status,
        ServiceStatus::Starting | ServiceStatus::Running | ServiceStatus::Stopping
    )
}
fn session_active(session: &ServiceSession) -> bool {
    session.services.iter().any(service_active)
}
fn api_cost(run: &Document) -> Option<f64> {
    let usage = run.get::<serde_json::Value>("usage").ok().flatten()?;
    if usage["costSource"] != "openai-standard-api"
        || usage["costScope"] != "requested-model-thread"
    {
        return None;
    }
    usage["estimatedCostUsd"]
        .as_f64()
        .filter(|amount| amount.is_finite() && (0.0..=1_000_000.0).contains(amount))
}
fn usage_text(run: &Document, i18n: I18n) -> String {
    let Some(usage) = run.get::<serde_json::Value>("usage").ok().flatten() else {
        return String::new();
    };
    let counter = |key: &str| {
        usage[key]
            .as_u64()
            .map(|count| count.to_string())
            .unwrap_or_else(|| "-".into())
    };
    format!(
        "{} {} / {} / {}",
        i18n.text("message.input_cache_output_tokens"),
        counter("inputTokens"),
        counter("cachedInputTokens"),
        counter("outputTokens")
    )
}
fn safe_link(url: &str) -> Option<String> {
    (url.starts_with("https://") || url.starts_with("http://")).then(|| url.to_owned())
}

/// Historical workspace panels from 7f48996, composed with the Rust application contracts.
#[component]
pub fn WorkspaceViews(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    view: Signal<WorkspaceView>,
    project: Signal<String>,
    on_open: Callback<String>,
    on_view: Callback<WorkspaceView>,
    on_focus: Callback<String>,
    on_projects: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let now = crate::time::clock();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let removing = RwSignal::new(None::<String>);
    let cards = Signal::derive(move || {
        let repo = project.get();
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        board.cards.with(|cards| {
            cards
                .iter()
                .filter(|card| {
                    !card.fields.flag("brainstormSession").unwrap_or(false)
                        && (view.get() == WorkspaceView::Processes
                            || repo.is_empty()
                            || same_repository(&field(&card.fields, "repo"), &repo, windows))
                        && (view.get() != WorkspaceView::Overview
                            || !card.fields.flag("archived").unwrap_or(false))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let runs = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .filter(|run| {
                    cards.with(|cards| cards.iter().any(|card| card.id == field(run, "cardId")))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let navigation = expect_context::<NavigationGuard>();
    navigation.1.update(|owner| *owner = owner.wrapping_add(1));
    let owner = navigation.1.get_untracked();
    let guard = navigation.0;
    guard.set(Some(Callback::new(
        move |continue_navigation: Callback<()>| {
            if busy.get_untracked() {
                error.set("message.wait_for_the_current_operation_to_finish".into());
            } else if removing.get_untracked().is_some() {
                error.set(
                    "message.confirm_or_cancel_worktree_removal_before_leaving_this_view".into(),
                );
            } else {
                continue_navigation.run(());
            }
        },
    )));
    on_cleanup(move || {
        if navigation.1.try_get_untracked() == Some(owner) {
            let _ = guard.try_set(None);
        }
    });
    let stop_run = Callback::new(move |id: String| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/runs/{id}/stop"),
                &serde_json::json!({}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                busy.set(false);
            }
        });
    });
    view! {
        <p class="inline-error" role="alert">
            {move || i18n.worktree_error(&error.get())}
        </p>
        <Show when={move || view.get() == WorkspaceView::Overview}>
            <Overview board cards runs now project on_open on_view on_focus />
        </Show>
        <Show when={move || view.get() == WorkspaceView::Agents}>
            <crate::queue::Queue board token on_open />
            <Agents board cards runs now busy on_open on_stop={stop_run} />
        </Show>
        <Show when={move || view.get() == WorkspaceView::Processes}>
            <Processes board token cards project busy error on_open on_projects />
        </Show>
        <Show when={move || view.get() == WorkspaceView::Worktrees}>
            <Worktrees board token cards runs busy error removing on_open />
        </Show>
    }
}

#[component]
fn Overview(
    board: BoardState,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    now: RwSignal<f64>,
    project: Signal<String>,
    on_open: Callback<String>,
    on_view: Callback<WorkspaceView>,
    on_focus: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let attention = Signal::derive(move || {
        cards.with(|cards| {
            cards
                .iter()
                .filter(|card| needs_attention(card, &runs.get(), cards))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let priority = Signal::derive(move || {
        let mut items = attention.get();
        for status in ["review", "todo"] {
            for card in cards
                .get()
                .into_iter()
                .filter(|card| field(&card.fields, "status") == status)
            {
                if !items.iter().any(|item| item.id == card.id) {
                    items.push(card);
                }
            }
        }
        items.truncate(6);
        items
    });
    view! {
        <For
            each={move || {
                cards
                    .get()
                    .into_iter()
                    .filter(|card| {
                        field(&card.fields, "status") != "done"
                            && matches!(
                                field(&nested(&card.fields, "automation"), "state").as_str(),
                                "ready" | "blocked" | "error" | "interrupted" | "cancelled"
                            )
                    })
                    .collect::<Vec<_>>()
            }}
            key={|card| serde_json::to_string(card).unwrap_or_default()}
            children={move |card| {
                let id = card.id.clone();
                let message = field(&nested(&card.fields, "automation"), "message");
                view! {
                    <article class="worktree-box">
                        <strong>{title(&card)}</strong>
                        <p>{i18n.text(&message)}</p>
                        <button data-open-card={id.clone()} on:click={move |_| on_open.run(id.clone())}>
                            {i18n.text("message.view_result")}
                        </button>
                        <PullRequest card />
                    </article>
                }
            }}
        />
        <div class="overview-metrics">
            <button
                data-go-view="board"
                data-focus-target="review"
                on:click={move |_| on_focus.run("review".into())}
            >
                <span>{i18n.text("message.to_review")}</span>
                <strong>
                    {move || {
                        cards
                            .with(|cards| {
                                cards.iter().filter(|card| field(&card.fields, "status") == "review").count()
                            })
                    }}
                </strong>
                <small>{i18n.text("message.review_completed_work")}</small>
            </button>
            <button
                data-go-view="board"
                data-focus-target="attention"
                on:click={move |_| on_focus.run("attention".into())}
            >
                <span>{i18n.text("message.to_resume")}</span>
                <strong>{move || attention.with(Vec::len)}</strong>
                <small>{i18n.text("message.examine_interruptions")}</small>
            </button>
            <button data-go-view="agents" on:click={move |_| on_view.run(WorkspaceView::Agents)}>
                <span>{i18n.text("message.active_agents")}</span>
                <strong>{move || runs.with(|runs| runs.iter().filter(|run| active(run)).count())}</strong>
                <small>{i18n.text("message.follow_runs")}</small>
            </button>
            <button data-go-view="time" on:click={move |_| on_view.run(WorkspaceView::Time)}>
                <span>{i18n.text("message.time_awaiting_approval")}</span>
                <strong>
                    {move || {
                        board
                            .times
                            .with(|entries| {
                                entries
                                    .iter()
                                    .filter(|entry| {
                                        board
                                            .cards
                                            .with(|cards| {
                                                cards
                                                    .iter()
                                                    .any(|card| {
                                                        card.id == field(entry, "cardId")
                                                            && (project.get().is_empty()
                                                                || same_repository(
                                                                    &field(&card.fields, "repo"),
                                                                    &project.get(),
                                                                    board
                                                                        .settings
                                                                        .with(|settings| field(settings, "platform") == "win32"),
                                                                ))
                                                    })
                                            }) && !entry.has_text("runningSince").unwrap_or(false)
                                            && field(entry, "exportState") != "exported"
                                    })
                                    .count()
                            })
                    }}
                </strong>
                <small>{i18n.text("message.review_before_sending_to_tempo")}</small>
            </button>
        </div>
        <div class="overview-columns">
            <section class="overview-section">
                <div class="panel-heading">
                    <h2>{i18n.text("message.your_next_step_2b7bb8fc")}</h2>
                </div>
                <For
                    each={move || priority.get()}
                    key={|card| serde_json::to_string(card).unwrap_or_default()}
                    children={move |card| {
                        let id = card.id.clone();
                        let label = if attention.with(|items| items.iter().any(|item| item.id == card.id)) {
                            "message.to_resume"
                        } else if field(&card.fields, "status") == "review" {
                            "message.to_review"
                        } else {
                            "message.to_do"
                        };
                        let ticket = field(&nested(&card.fields, "ticket"), "key");
                        let criteria = card
                            .fields
                            .get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        view! {
                            <button
                                class="next-task"
                                data-open-card={id.clone()}
                                on:click={move |_| on_open.run(id.clone())}
                            >
                                <span class="state-badge">{i18n.text(label)}</span>
                                <strong>{title(&card)}</strong>
                                <small>
                                    {if ticket.is_empty() {
                                        i18n.text("message.local_task_71560e02")
                                    } else {
                                        ticket
                                    }}
                                    {if criteria.is_empty() {
                                        String::new()
                                    } else {
                                        i18n.text("message.0_1_criteria_approved")
                                            .replace(
                                                "{0}",
                                                &criteria.iter().filter(|item| item.done).count().to_string(),
                                            )
                                            .replace("{1}", &criteria.len().to_string())
                                    }}
                                </small>
                            </button>
                        }
                    }}
                />
                <p class="empty" hidden={move || !priority.with(Vec::is_empty)}>
                    {i18n.text("message.nothing_to_resume_or_review_you_can_prepare_what_comes_next")}
                </p>
            </section>
            <section class="overview-section">
                <div class="panel-heading">
                    <h2>{i18n.text("message.recent_runs")}</h2>
                </div>
                <For
                    each={move || runs.get().into_iter().take(5).collect::<Vec<_>>()}
                    key={|run| serde_json::to_string(run).unwrap_or_default()}
                    children={move |run| {
                        let id = field(&run, "cardId");
                        let name = cards
                            .with(|cards| {
                                cards.iter().find(|card| card.id == id).map(title).unwrap_or_default()
                            });
                        let current = StoredValue::new(run);
                        view! {
                            <button
                                class="recent-run"
                                data-open-card={id.clone()}
                                on:click={move |_| on_open.run(id.clone())}
                            >
                                <span class={format!(
                                    "agent-pill {}",
                                    field(&current.get_value(), "agent"),
                                )}>{agent_name(&board.settings.get_untracked(), &current.get_value())}</span>
                                <strong>{name}</strong>
                                <small data-agent-state={field(
                                    &current.get_value(),
                                    "id",
                                )}>{move || i18n.text(agent_state(&current.get_value(), now.get()))}</small>
                                <small data-clock={field(
                                    &current.get_value(),
                                    "id",
                                )}>{move || elapsed(&current.get_value(), now.get())}</small>
                            </button>
                        }
                    }}
                />
                <p class="empty" hidden={move || !runs.with(Vec::is_empty)}>
                    {i18n.text("message.your_agents_will_appear_here_from_their_first_task")}
                </p>
                <button
                    class="text-button"
                    data-go-view="worktrees"
                    on:click={move |_| on_view.run(WorkspaceView::Worktrees)}
                >
                    {i18n.text("message.view_branches_worktrees_and_prs")}
                </button>
            </section>
        </div>
    }
}

#[component]
fn Agents(
    board: BoardState,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    now: RwSignal<f64>,
    busy: RwSignal<bool>,
    on_open: Callback<String>,
    on_stop: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        <div class="view-stats">
            {[
                ("message.in_progress", "message.agents_at_work", 0),
                ("message.completed", "message.runs_to_review", 1),
                ("message.needs_review", "message.errors_or_interruptions", 2),
            ]
                .into_iter()
                .map(move |(label, help, kind)| {
                    view! {
                        <div>
                            <span>{i18n.text(label)}</span>
                            <strong>
                                {move || {
                                    runs
                                        .with(|runs| {
                                            runs
                                                .iter()
                                                .filter(|run| match kind {
                                                    0 => active(run),
                                                    1 => field(run, "status") == "completed",
                                                    _ => {
                                                        matches!(
                                                            field(run, "status").as_str(),
                                                            "failed" | "cancelled" | "interrupted"
                                                        )
                                                    }
                                                })
                                                .count()
                                        })
                                }}
                            </strong>
                            <small>{i18n.text(help)}</small>
                        </div>
                    }
                })
                .collect_view()}
        </div>
        <div class="panel-heading">
            <div>
                <h2>{i18n.text("message.your_runs")}</h2>
                <p class="hint">
                    {i18n.text("message.follow_work_in_progress_or_open_a_task_to_view_its_result")}
                </p>
            </div>
        </div>
        <details class="view-help">
            <summary>{i18n.text("message.an_agent_is_no_longer_responding")}</summary>
            <p class="hint">
                {i18n.text("message.silence_does_not_prove_a_blockage_after_a_permission_denial_adju")}
            </p>
        </details>
        <section class="worktree-box">
            <h3>{i18n.text("message.approximate_api_cost_and_agent_time")}</h3>
            <p>
                <strong>
                    {move || {
                        let costs = runs.with(|runs| runs.iter().filter_map(api_cost).collect::<Vec<_>>());
                        if costs.is_empty() {
                            i18n.text("message.unavailable")
                        } else {
                            format!("{:.4} USD", costs.iter().sum::<f64>())
                        }
                    }}
                </strong>
                " "
                {move || {
                    i18n
                        .text("message.across_0_run_s_with_cost_total_time")
                        .replace(
                            "{0}",
                            &runs
                                .with(|runs| runs.iter().filter(|run| api_cost(run).is_some()).count())
                                .to_string(),
                        )
                }}
                " "
                <strong>
                    {move || duration_text(
                        runs
                            .with(|runs| {
                                runs
                                    .iter()
                                    .filter_map(|run| run.get::<u64>("activeMilliseconds").ok().flatten())
                                    .fold(0u64, u64::saturating_add)
                            }) / 1000,
                    )}
                </strong>
                " "
                {move || i18n.text("message.across_0_run_s").replace("{0}", &runs.with(Vec::len).to_string())}
            </p>
            <p class="hint">
                {move || {
                    i18n
                        .text("message.0_run_s_without_cost_data_current_scope_all_dates_indicative_api")
                        .replace(
                            "{0}",
                            &runs
                                .with(|runs| runs.iter().filter(|run| api_cost(run).is_none()).count())
                                .to_string(),
                        )
                }}
            </p>
            <crate::agent_usage::AgentUsage runs />
        </section>
        <For
            each={move || runs.get()}
            key={|run| serde_json::to_string(run).unwrap_or_default()}
            children={move |run| {
                let card_id = field(&run, "cardId");
                let run_id = field(&run, "id");
                let data = StoredValue::new(run);
                let open = card_id.clone();
                let stop = run_id.clone();
                view! {
                    <article class="workspace-row agent-run" data-status={field(&data.get_value(), "status")}>
                        <div>
                            <span class={format!(
                                "agent-pill {}",
                                field(&data.get_value(), "agent"),
                            )}>{agent_name(&board.settings.get_untracked(), &data.get_value())}</span>
                            <h3>
                                {cards
                                    .with(|cards| {
                                        cards
                                            .iter()
                                            .find(|card| card.id == card_id)
                                            .map(title)
                                            .unwrap_or_default()
                                    })}
                            </h3>
                            <p>
                                <span class="state-badge" data-agent-state={run_id.clone()}>
                                    {move || i18n.text(agent_state(&data.get_value(), now.get()))}
                                </span>
                                " "
                                <span data-clock={run_id}>
                                    {move || elapsed(&data.get_value(), now.get())}
                                </span>
                            </p>
                            <p class="hint">
                                {
                                    let attention = field(&data.get_value(), "attention");
                                    if attention.is_empty() {
                                        field(&data.get_value(), "activity")
                                    } else {
                                        i18n.text(&attention)
                                    }
                                }
                            </p>
                            <p class="hint">{usage_text(&data.get_value(), i18n)}</p>
                            <code>
                                {
                                    let branch = field(&data.get_value(), "branch");
                                    if branch.is_empty() { field(&data.get_value(), "repo") } else { branch }
                                }
                            </code>
                        </div>
                        <div class="row-actions">
                            <button
                                data-open-card={open.clone()}
                                on:click={move |_| on_open.run(open.clone())}
                            >
                                {i18n.text("message.open_task")}
                            </button>
                            <button
                                class="danger"
                                hidden={!active(&data.get_value())}
                                disabled={move || busy.get()}
                                data-stop-agent={stop.clone()}
                                on:click={move |_| on_stop.run(stop.clone())}
                            >
                                {i18n.text("message.stop")}
                            </button>
                        </div>
                    </article>
                }
            }}
        />
        <p class="empty" hidden={move || !runs.with(Vec::is_empty)}>
            {i18n.text("message.no_agent_started_for_this_project")}
        </p>
    }
}

#[component]
fn PullRequest(card: Card) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let pr = nested(&card.fields, "pullRequest");
    let pr = if pr.keys().next().is_none() {
        nested(&nested(&card.fields, "connectorPullRequest"), "pullRequest")
    } else {
        pr
    };
    let url = safe_link(&field(&pr, "url"));
    url.map(move |url| {
        view! {
            <p>
                <a href={url} target="_blank" rel="noopener noreferrer">
                    {i18n.text("message.open_pr")}
                </a>
            </p>
        }
    })
}

#[component]
fn Processes(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    cards: Signal<Vec<Card>>,
    project: Signal<String>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    on_open: Callback<String>,
    on_projects: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let sessions = RwSignal::new(Vec::<ServiceSession>::new());
    let selected = RwSignal::new(BTreeMap::<String, String>::new());
    let loading = Rc::new(Cell::new(false));
    let alive = Rc::new(Cell::new(true));
    let visible_projects = Signal::derive(move || {
        board
            .projects
            .get()
            .into_iter()
            .filter(|item| project.get().is_empty() || field(item, "repo") == project.get())
            .collect::<Vec<_>>()
    });
    let visible_sessions = Signal::derive(move || {
        let repo = project.get();
        if repo.is_empty() {
            return sessions.get();
        }
        let projects = board.projects.get();
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        let selected = projects
            .iter()
            .find(|item| same_repository(&field(item, "repo"), &repo, windows))
            .map(|item| field(item, "id"));
        sessions
            .get()
            .into_iter()
            .filter(|session| {
                selected
                    .as_ref()
                    .is_some_and(|id| session.project_ids.contains(id))
                    || cards.with(|cards| {
                        cards
                            .iter()
                            .find(|card| card.id == session.card_id)
                            .is_some_and(|card| {
                                same_repository(&field(&card.fields, "repo"), &repo, windows)
                                    || selected.as_ref().is_some_and(|id| {
                                        projects.iter().any(|item| {
                                            same_repository(
                                                &field(item, "repo"),
                                                &field(&card.fields, "repo"),
                                                windows,
                                            ) && field(item, "linkedProjectId") == *id
                                        })
                                    })
                            })
                    })
            })
            .collect::<Vec<_>>()
    });
    Effect::new(move |_| {
        let cards = cards.get();
        let sessions = sessions.get();
        let projects = visible_projects.get();
        let mut next = selected.get_untracked();
        let windows = board
            .settings
            .with_untracked(|settings| field(settings, "platform") == "win32");
        for item in projects {
            let id = field(&item, "id");
            let repo = field(&item, "repo");
            let valid = |candidate: &str| {
                cards.iter().any(|card| {
                    card.id == candidate
                        && card.fields.raw("worktree").is_some()
                        && !card.fields.flag("archived").unwrap_or(false)
                        && same_repository(&field(&card.fields, "repo"), &repo, windows)
                })
            };
            if next.get(&id).is_some_and(|candidate| !valid(candidate)) {
                next.remove(&id);
            }
            if !next.contains_key(&id)
                && let Some(session) = sessions.iter().find(|session| {
                    session.project_ids.contains(&id)
                        && session_active(session)
                        && valid(&session.card_id)
                })
            {
                next.insert(id, session.card_id.clone());
            }
        }
        if next != selected.get_untracked() {
            selected.set(next);
        }
    });
    let refresh = {
        let loading = loading.clone();
        let alive = alive.clone();
        Rc::new(move || {
            if !alive.get() || loading.replace(true) {
                return;
            }
            let Ok(credential) = token.get_value() else {
                loading.set(false);
                return;
            };
            let targets = cards
                .get_untracked()
                .into_iter()
                .map(|card| card.id)
                .collect::<Vec<_>>();
            let loading = loading.clone();
            let alive = alive.clone();
            spawn_local(async move {
                let mut next = Vec::new();
                let mut failures = Vec::new();
                for id in targets {
                    let result = async {
                        read(&credential, &format!("/api/cards/{id}/services"))
                            .await?
                            .json::<Option<ServiceSession>>()
                            .await
                            .map_err(|_| "message.invalid_server_response".to_owned())
                    }
                    .await;
                    match result {
                        Ok(Some(session)) => next.push(session),
                        Ok(None) => {}
                        Err(message) => failures.push(message),
                    }
                }
                if alive.get() {
                    sessions.set(next);
                    if !failures.is_empty() {
                        error.set(failures.join("\n"));
                    }
                }
                loading.set(false);
            });
        })
    };
    {
        let refresh = refresh.clone();
        Effect::new(move |_| {
            let _ = cards.get();
            refresh();
        });
    }
    let timer_refresh = refresh.clone();
    let tick = Closure::<dyn FnMut()>::new(move || timer_refresh());
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                3000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((interval, tick));
    let cleanup_alive = StoredValue::new_local(alive);
    on_cleanup(move || {
        cleanup_alive.with_value(|alive| alive.set(false));
        timer.with_value(|(interval, _)| {
            if let (Some(window), Some(id)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(id);
            }
        });
    });
    let action = {
        let refresh = StoredValue::new_local(refresh.clone());
        Callback::new(
            move |operations: Vec<(String, &'static str, Option<String>)>| {
                if busy.get_untracked() || operations.is_empty() {
                    return;
                }
                let Ok(credential) = token.get_value() else {
                    return;
                };
                busy.set(true);
                error.set(String::new());
                let refresh = refresh.get_value();
                spawn_local(async move {
                    let mut failures = Vec::new();
                    for (id, operation, service) in operations {
                        let mut body = serde_json::json!({"standalone":true});
                        if let Some(service) = service {
                            body["serviceId"] = serde_json::json!(service);
                        }
                        if let Err(message) = write(
                            &credential,
                            Method::POST,
                            &format!("/api/cards/{id}/services/{operation}"),
                            &body,
                        )
                        .await
                        {
                            failures.push(message);
                        }
                    }
                    let _ = board.refresh(&credential).await;
                    if busy.try_get_untracked().is_some() {
                        busy.set(false);
                        error.set(failures.join("\n"));
                        refresh();
                    }
                });
            },
        )
    };
    let start_selected = move || {
        selected
            .get_untracked()
            .into_iter()
            .filter(|(project_id, id)| {
                visible_projects.with_untracked(|projects| {
                    projects
                        .iter()
                        .any(|project| field(project, "id") == *project_id)
                }) && !sessions.with(|sessions| {
                    sessions
                        .iter()
                        .any(|session| session.card_id == *id && session_active(session))
                })
            })
            .map(|(_, id)| (id, "start", None))
            .collect::<Vec<_>>()
    };
    let manual_refresh = refresh.clone();
    view! {
        <div class="panel-heading">
            <div>
                <h2>{i18n.text("message.launch_projects")}</h2>
                <p class="hint">
                    {i18n.text("message.choose_a_worktree_for_each_project_its_front_and_back_commands_s")}
                </p>
            </div>
            <div class="row-actions">
                <button
                    id="start-selected-projects"
                    class="primary"
                    disabled={move || busy.get() || start_selected().is_empty()}
                    on:click={move |_| action.run(start_selected())}
                >
                    {i18n.text("message.launch_selected")}
                </button>
                <button
                    id="refresh-processes"
                    disabled={move || busy.get()}
                    on:click={move |_| manual_refresh()}
                >
                    {i18n.text("message.refresh")}
                </button>
                <button
                    id="stop-visible-processes"
                    class="danger"
                    disabled={move || {
                        busy.get() || !visible_sessions.with(|sessions| sessions.iter().any(session_active))
                    }}
                    on:click={move |_| {
                        action
                            .run(
                                visible_sessions
                                    .get_untracked()
                                    .iter()
                                    .filter(|session| session_active(session))
                                    .map(|session| (session.card_id.clone(), "stop", None))
                                    .collect(),
                            )
                    }}
                >
                    {i18n.text("message.stop_everything_in_this_view")}
                </button>
            </div>
        </div>
        <For
            each={move || visible_projects.get()}
            key={|item| serde_json::to_string(item).unwrap_or_default()}
            children={move |item| {
                let id = StoredValue::new(field(&item, "id"));
                let repo = field(&item, "repo");
                let choices = Signal::derive(move || {
                    cards
                        .get()
                        .into_iter()
                        .filter(|card| {
                            card.fields.raw("worktree").is_some()
                                && !card.fields.flag("archived").unwrap_or(false)
                                && same_repository(
                                    &field(&card.fields, "repo"),
                                    &repo,
                                    board.settings.with(|settings| field(settings, "platform") == "win32"),
                                )
                        })
                        .collect::<Vec<_>>()
                });
                let current = Signal::derive(move || {
                    selected.with(|selected| selected.get(&id.get_value()).cloned().unwrap_or_default())
                });
                let configured = !field(&item, "frontCommand").is_empty()
                    || !field(&item, "backCommand").is_empty();
                let managed = [field(&item, "frontCommand"), field(&item, "backCommand")]
                    .iter()
                    .filter(|command| !command.is_empty())
                    .all(|command| command.contains("{port}"));
                let occupied = move || {
                    sessions
                        .with(|sessions| {
                            sessions
                                .iter()
                                .any(|session| {
                                    session.project_ids.contains(&id.get_value()) && session_active(session)
                                        && (!managed
                                            || session.services.iter().any(|service| service.port.is_none()))
                                })
                        })
                };
                view! {
                    <article class="workspace-row">
                        <div>
                            <h3>{field(&item, "name")}</h3>
                            <p class="hint">
                                {if configured {
                                    [
                                        (!field(&item, "frontCommand").is_empty())
                                            .then(|| i18n.text("message.front")),
                                        (!field(&item, "backCommand").is_empty())
                                            .then(|| i18n.text("message.back_76900f1b")),
                                    ]
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>()
                                        .join(" + ")
                                } else {
                                    i18n.text("message.enter_commands_in_settings")
                                }}
                            </p>
                            <label>
                                {i18n.text("message.worktree")}
                                <select
                                    data-launch-project={id.get_value()}
                                    disabled={move || busy.get() || occupied() || !configured}
                                    prop:value={move || current.get()}
                                    on:change={move |event| {
                                        selected
                                            .update(|selected| {
                                                let value = event_target_value(&event);
                                                if value.is_empty() {
                                                    selected.remove(&id.get_value());
                                                } else {
                                                    selected.insert(id.get_value(), value);
                                                }
                                            })
                                    }}
                                >
                                    <option value="">
                                        {move || {
                                            if choices.with(Vec::is_empty) {
                                                i18n.text("message.prepare_a_worktree_from_a_card")
                                            } else {
                                                i18n.text("message.do_not_launch")
                                            }
                                        }}
                                    </option>
                                    <For
                                        each={move || choices.get()}
                                        key={|card| serde_json::to_string(card).unwrap_or_default()}
                                        children={move |card| {
                                            let branch = card
                                                .fields
                                                .get::<Worktree>("worktree")
                                                .ok()
                                                .flatten()
                                                .map(|worktree| worktree.branch)
                                                .unwrap_or_else(|| title(&card));
                                            view! {
                                                <option value={card
                                                    .id
                                                    .clone()}>{format!("{branch} · {}", title(&card))}</option>
                                            }
                                        }}
                                    />
                                </select>
                            </label>
                            <p class="path-text">
                                {move || {
                                    choices
                                        .with(|cards| {
                                            cards
                                                .iter()
                                                .find(|card| card.id == current.get())
                                                .and_then(|card| {
                                                    card.fields.get::<Worktree>("worktree").ok().flatten()
                                                })
                                                .map(|worktree| worktree.path)
                                                .unwrap_or_default()
                                        })
                                }}
                            </p>
                        </div>
                        <div class="row-actions">
                            <button
                                data-launch-one={id.get_value()}
                                disabled={move || {
                                    busy.get() || occupied() || !configured || current.get().is_empty()
                                }}
                                on:click={move |_| action.run(vec![(current.get_untracked(), "start", None)])}
                            >
                                {i18n.text("message.launch")}
                            </button>
                            <button
                                data-launch-settings={id.get_value()}
                                disabled={move || busy.get()}
                                on:click={move |_| on_projects.run(())}
                            >
                                {i18n.text("message.configure_launch")}
                            </button>
                        </div>
                    </article>
                }
            }}
        />
        <h3>
            {move || {
                i18n
                    .text("message.services_and_logs_0_running")
                    .replace(
                        "{0}",
                        &visible_sessions
                            .with(|sessions| {
                                sessions
                                    .iter()
                                    .flat_map(|session| &session.services)
                                    .filter(|service| matches!(service.status, ServiceStatus::Running))
                                    .count()
                            })
                            .to_string(),
                    )
            }}
        </h3>
        <For
            each={move || {
                cards
                    .get()
                    .into_iter()
                    .filter(|card| {
                        visible_sessions
                            .with(|sessions| sessions.iter().any(|session| session.card_id == card.id))
                    })
                    .collect::<Vec<_>>()
            }}
            key={|card| serde_json::to_string(card).unwrap_or_default()}
            children={move |card| {
                let id = StoredValue::new(card.id.clone());
                let current = Signal::derive(move || {
                    sessions
                        .with(|sessions| {
                            sessions.iter().find(|session| session.card_id == id.get_value()).cloned()
                        })
                });
                let owner = board
                    .projects
                    .with_untracked(|projects| {
                        projects
                            .iter()
                            .find(|project| same_repository(
                                &field(project, "repo"),
                                &field(&card.fields, "repo"),
                                board
                                    .settings
                                    .with_untracked(|settings| field(settings, "platform") == "win32"),
                            ))
                            .cloned()
                    });
                let ready = card.fields.raw("worktree").is_some()
                    && !card.fields.flag("archived").unwrap_or(false)
                    && owner
                        .as_ref()
                        .is_some_and(|project| {
                            !field(project, "frontCommand").is_empty()
                                || !field(project, "backCommand").is_empty()
                        });
                let branch = card
                    .fields
                    .get::<Worktree>("worktree")
                    .ok()
                    .flatten()
                    .map(|worktree| worktree.branch)
                    .unwrap_or_else(|| i18n.text("message.card_worktree"));
                view! {
                    <article class="process-stack">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">
                                    {owner
                                        .map(|owner| field(&owner, "name"))
                                        .unwrap_or_else(|| i18n.text("message.project_removed"))}
                                </span>
                                <h3>{title(&card)}</h3>
                                <p class="hint">
                                    {branch}
                                    {if card.fields.flag("archived").unwrap_or(false) {
                                        i18n.text("message.archived_card")
                                    } else {
                                        String::new()
                                    }}
                                </p>
                            </div>
                            <div class="row-actions">
                                <button
                                    data-open-card={id.get_value()}
                                    disabled={move || busy.get()}
                                    on:click={move |_| on_open.run(id.get_value())}
                                >
                                    {i18n.text("message.view_card")}
                                </button>
                                <button
                                    data-process-action="start"
                                    data-process-card={id.get_value()}
                                    disabled={move || {
                                        busy.get() || !ready
                                            || current
                                                .with(|session| session.as_ref().is_some_and(session_active))
                                    }}
                                    on:click={move |_| action.run(vec![(id.get_value(), "start", None)])}
                                >
                                    {i18n.text("message.start_all")}
                                </button>
                                <button
                                    class="danger"
                                    data-process-action="stop"
                                    data-process-card={id.get_value()}
                                    disabled={move || {
                                        busy.get()
                                            || current
                                                .with(|session| {
                                                    session
                                                        .as_ref()
                                                        .is_none_or(|session| !session_active(session))
                                                })
                                    }}
                                    on:click={move |_| action.run(vec![(id.get_value(), "stop", None)])}
                                >
                                    {i18n.text("message.stop_all")}
                                </button>
                            </div>
                        </div>
                        <div class="process-services">
                            <For
                                each={move || {
                                    current.get().map(|session| session.services).unwrap_or_default()
                                }}
                                key={|service| service.id.clone()}
                                children={move |service| {
                                    let service_id = StoredValue::new(service.id.clone());
                                    let service = Signal::derive(move || {
                                        current
                                            .get()
                                            .and_then(|session| {
                                                session
                                                    .services
                                                    .into_iter()
                                                    .find(|service| service.id == service_id.get_value())
                                            })
                                    });
                                    view! {
                                        <ProcessService service card_id={id} busy ready on_action={action} />
                                    }
                                }}
                            />
                        </div>
                        <p class="hint" hidden={ready}>
                            {i18n.text("message.worktree_missing_prepare_it_from_the_card_before_restarting")}
                        </p>
                        <details class="process-journal" data-process-journal={id.get_value()}>
                            <summary>{i18n.text("message.combined_log")}</summary>
                            <pre class="log" data-process-log={id.get_value()}>
                                {move || {
                                    current
                                        .get()
                                        .map(|session| {
                                            session
                                                .services
                                                .into_iter()
                                                .map(|service| format!("{}\n{}", service.name, service.log))
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        })
                                        .unwrap_or_default()
                                }}
                            </pre>
                        </details>
                    </article>
                }
            }}
        />
        <p class="empty" hidden={move || !visible_sessions.with(Vec::is_empty)}>
            {i18n.text("message.no_services_started_prepare_a_cards_worktree_then_select_it_abov")}
        </p>
    }
}

#[component]
fn ProcessService(
    service: Signal<Option<ServiceState>>,
    card_id: StoredValue<String>,
    busy: RwSignal<bool>,
    ready: bool,
    on_action: Callback<Vec<(String, &'static str, Option<String>)>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        <section class="process-service">
            <div class="panel-heading">
                <div>
                    <h4>{move || service.get().map(|service| service.name).unwrap_or_default()}</h4>
                    <span
                        class="state-badge"
                        data-status={move || {
                            service
                                .get()
                                .and_then(|service| serde_json::to_value(service.status).ok())
                                .and_then(|value| value.as_str().map(str::to_owned))
                                .unwrap_or_default()
                        }}
                    >
                        {move || {
                            service
                                .get()
                                .map(|service| match service.status {
                                    ServiceStatus::Starting | ServiceStatus::Running => {
                                        i18n.text("message.in_progress")
                                    }
                                    ServiceStatus::Stopping => i18n.text("message.stopping_1cbeb899"),
                                    _ => {
                                        i18n.text("message.stopped_0")
                                            .replace(
                                                "{0}",
                                                &service
                                                    .exit_code
                                                    .map(|code| {
                                                        i18n
                                                            .text("message.code_0")
                                                            .replace("{0}", &code.to_string())
                                                    })
                                                    .unwrap_or_default(),
                                            )
                                    }
                                })
                                .unwrap_or_default()
                        }}
                    </span>
                </div>
                <div class="row-actions">
                    {[("start", "message.start"), ("restart", "message.restart"), ("stop", "message.stop")]
                        .into_iter()
                        .map(move |(operation, label)| {
                            view! {
                                <button
                                    class={if operation == "stop" { "danger" } else { "" }}
                                    data-process-action={operation}
                                    data-process-card={card_id.get_value()}
                                    data-service-id={move || {
                                        service.get().map(|service| service.id).unwrap_or_default()
                                    }}
                                    disabled={move || {
                                        busy.get()
                                            || service
                                                .with(|service| {
                                                    service
                                                        .as_ref()
                                                        .is_none_or(|service| {
                                                            if operation == "stop" {
                                                                !service_active(service)
                                                            } else if operation == "restart" {
                                                                !ready || !matches!(service.status, ServiceStatus::Running)
                                                            } else {
                                                                !ready || service_active(service)
                                                            }
                                                        })
                                                })
                                    }}
                                    on:click={move |_| {
                                        if let Some(service) = service.get_untracked() {
                                            on_action
                                                .run(
                                                    vec![
                                                        (
                                                            card_id.get_value(),
                                                            if operation == "start" { "restart" } else { operation },
                                                            Some(service.id),
                                                        ),
                                                    ],
                                                );
                                        }
                                    }}
                                >
                                    {i18n.text(label)}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </div>
            <p class="path-text">
                {move || service.get().map(|service| service.directory).unwrap_or_default()}
            </p>
            <p class="hint">
                {i18n.text("message.port_assigned_by_atelier")}" "
                <code>
                    {move || {
                        service
                            .get()
                            .and_then(|service| service.port)
                            .map(|port| port.to_string())
                            .unwrap_or_else(|| "-".into())
                    }}
                </code>" "{i18n.text("message.launcher_pid")}" "
                <code>
                    {move || {
                        service
                            .get()
                            .and_then(|service| service.pid)
                            .map(|pid| pid.to_string())
                            .unwrap_or_else(|| "-".into())
                    }}
                </code>
                {move || {
                    service
                        .get()
                        .map(|service| {
                            if service.process_ids.len() > 1 {
                                i18n.text("message.child_processes_0")
                                    .replace(
                                        "{0}",
                                        &service
                                            .process_ids
                                            .iter()
                                            .filter(|pid| Some(**pid) != service.pid)
                                            .map(u32::to_string)
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                    )
                            } else {
                                String::new()
                            }
                        })
                        .unwrap_or_default()
                }}
            </p>
            <p class="inline-error">
                {move || {
                    service
                        .get()
                        .map(|service| {
                            [service.error, service.inspection_error]
                                .into_iter()
                                .filter(|error| !error.is_empty())
                                .map(|error| i18n.text(&error))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                }}
            </p>
            <div
                class="process-ports"
                hidden={move || {
                    service
                        .with(|service| service.as_ref().is_none_or(|service| service.endpoints.is_empty()))
                }}
            >
                <table>
                    <caption class="hint">{i18n.text("message.listening_tcp_addresses")}</caption>
                    <thead>
                        <tr>
                            <th>{i18n.text("message.ip_address")}</th>
                            <th>{i18n.text("message.port")}</th>
                            <th>"PID"</th>
                            <th>{i18n.text("message.application")}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <For
                            each={move || service.get().map(|service| service.endpoints).unwrap_or_default()}
                            key={|endpoint| (endpoint.pid, endpoint.address.clone(), endpoint.port)}
                            children={move |endpoint| {
                                view! {
                                    <tr>
                                        <td>
                                            <code>{endpoint.address.clone()}</code>
                                            {if ["0.0.0.0", "::"].contains(&endpoint.address.as_str()) {
                                                i18n.text("message.all_interfaces")
                                            } else {
                                                String::new()
                                            }}
                                        </td>
                                        <td>{endpoint.port}</td>
                                        <td>{endpoint.pid}</td>
                                        <td>
                                            {endpoint
                                                .url
                                                .and_then(|url| safe_link(&url))
                                                .map(move |url| {
                                                    let protocol = if url.starts_with("https:") {
                                                        "HTTPS"
                                                    } else {
                                                        "HTTP"
                                                    };
                                                    view! {
                                                        <a href={url} target="_blank" rel="noopener noreferrer">
                                                            {i18n.text("message.open_0").replace("{0}", protocol)}
                                                        </a>
                                                    }
                                                })}
                                        </td>
                                    </tr>
                                }
                            }}
                        />
                    </tbody>
                </table>
            </div>
            <p
                class="hint"
                hidden={move || {
                    service
                        .with(|service| service.as_ref().is_some_and(|service| !service.endpoints.is_empty()))
                }}
            >
                {move || {
                    if service.with(|service| service.as_ref().is_some_and(service_active)) {
                        i18n.text("message.no_listening_tcp_ports_detected_yet")
                    } else {
                        i18n.text("message.no_listening_ports")
                    }
                }}
            </p>
        </section>
    }
}

fn cleanup_candidate(card: &Card) -> bool {
    card.fields.raw("worktree").is_some()
        && (field(&card.fields, "status") == "done"
            || card.fields.flag("archived").unwrap_or(false)
            || field(&nested(&card.fields, "pullRequest"), "status") == "completed")
}

#[component]
fn Worktrees(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    removing: RwSignal<Option<String>>,
    on_open: Callback<String>,
) -> impl IntoView {
    use atelier_core::contracts::{
        ForgeConnections, PullRequest, PullRequestAttach, WorktreeInspection,
    };
    let i18n = expect_context::<I18n>();
    let checks = RwSignal::new(BTreeMap::<String, Result<WorktreeInspection, String>>::new());
    let force = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let inspect = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let selected = cards
            .get_untracked()
            .into_iter()
            .filter(|card| card.fields.raw("worktree").is_some())
            .collect::<Vec<_>>();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let mut next = BTreeMap::new();
            for card in selected {
                let result = async {
                    read(
                        &credential,
                        &format!("/api/cards/{}/worktree/inspection", card.id),
                    )
                    .await?
                    .json::<WorktreeInspection>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
                }
                .await;
                next.insert(card.id, result);
            }
            if busy.try_get_untracked().is_some() {
                checks.set(next);
                busy.set(false);
            }
        });
    });
    let refresh_prs = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let selected = cards
            .get_untracked()
            .into_iter()
            .filter(|card| {
                card.fields.raw("pullRequest").is_some()
                    || card.fields.raw("connectorPullRequest").is_some()
            })
            .collect::<Vec<_>>();
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            let mut failures = Vec::new();
            let configurations = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            for card in selected {
                let result=async{
                    if let Some(pr)=card.fields.get::<PullRequest>("pullRequest").map_err(|_|"message.invalid_pr_mr_link".to_owned())?{
                        let configuration=configurations.as_ref().map_err(Clone::clone)?;
                        if !configuration.connections.iter().any(|connection|connection.configuration.kind==pr.kind&&connection.configuration.base==pr.base_url){return Err("message.reconnect_this_pr_mr_s_forge_site_before_refreshing_it".into());}
                        let body=serde_json::to_value(PullRequestAttach{expected:atelier_core::application::jira::card_snapshot(&card),revision:configuration.revision,kind:pr.kind,repository:pr.repository,number:pr.number,confirmed:true}).map_err(|_|"message.invalid_pr_mr_link".to_owned())?;
                        write(&credential,Method::POST,&format!("/api/cards/{}/forge/attach",card.id),&body).await?;
                    }else if let Some(link)=card.fields.get::<atelier_core::application::extensions::ModulePullRequestLink>("connectorPullRequest").map_err(|_|"message.invalid_pr_mr_link".to_owned())?{
                        let input=serde_json::json!({"expectedRevision":link.revision,"method":"repositoriesRefresh","action":null,"confirmed":true,"input":{"expected":card,"target":link.pull_request.target_branch}});
                        write(&credential,Method::POST,&format!("/api/extensions/{}/invoke",link.module),&input).await?;
                    }
                    Ok::<(),String>(())
                }.await;
                if let Err(message) = result {
                    failures.push(format!("{} : {message}", title(&card)));
                }
            }
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                busy.set(false);
                if failures.is_empty() {
                    notice.set("message.pr_statuses_refreshed".into());
                } else {
                    error.set(failures.join("\n"));
                }
            }
        });
    });
    let remove = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(id), Ok(credential)) = (removing.get_untracked(), token.get_value()) else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        let discard = force.get_untracked();
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/cards/{id}/worktree/remove"),
                &serde_json::json!({"confirmed":true,"discardChanges":discard}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                busy.set(false);
                match result {
                    Ok(_) => {
                        removing.set(None);
                        checks.update(|checks| {
                            checks.remove(&id);
                        });
                        notice.set("message.worktrees_removed_branches_kept".into());
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let check_label = move |id: &str, cleanup: bool| {
        checks.with(|checks| match checks.get(id) {
            Some(Ok(check)) => i18n.text(if check.clean {
                if cleanup {
                    "message.main_folder_is_clean"
                } else {
                    "message.clean_folder"
                }
            } else if cleanup {
                "message.local_changes_to_keep"
            } else {
                "message.local_changes"
            }),
            Some(Err(message)) => i18n.text(message),
            None => i18n.text("message.git_status_not_checked"),
        })
    };
    view! {
        <div class="panel-heading">
            <div>
                <h2>{i18n.text("message.working_folders")}</h2>
                <p class="hint">
                    {i18n.text("message.your_branches_their_working_folders_and_associated_pull_requests")}
                </p>
            </div>
            <div class="row-actions">
                <button
                    id="refresh-prs"
                    disabled={move || busy.get()}
                    on:click={move |_| refresh_prs.run(())}
                >
                    {i18n.text("message.refresh_prs")}
                </button>
                <button
                    id="inspect-worktrees"
                    disabled={move || busy.get()}
                    on:click={move |_| inspect.run(())}
                >
                    {i18n.text("message.refresh_git_status")}
                </button>
            </div>
        </div>
        <p role="status">{move || i18n.text(&notice.get())}</p>
        <details class="disclosure">
            <summary>
                {i18n.text("message.clean_up_after_delivery")}" "
                <span class="count">
                    {move || cards.with(|cards| cards.iter().filter(|card| cleanup_candidate(card)).count())}
                </span>
            </summary>
            <p class="hint">
                {i18n.text("message.completed_or_archived_tasks_or_merged_prs_refresh_git_status_bef")}
            </p>
            <For
                each={move || cards.get().into_iter().filter(cleanup_candidate).collect::<Vec<_>>()}
                key={|card| serde_json::to_string(card).unwrap_or_default()}
                children={move |card| {
                    let id = StoredValue::new(card.id.clone());
                    view! {
                        <article class="workspace-row">
                            <div>
                                <h3>{title(&card)}</h3>
                                <p class="hint">{move || check_label(&id.get_value(), true)}</p>
                            </div>
                            <button
                                class="danger"
                                data-clean-candidate={id.get_value()}
                                disabled={move || {
                                    busy.get()
                                        || runs
                                            .with(|runs| {
                                                runs
                                                    .iter()
                                                    .any(|run| {
                                                        field(run, "cardId") == id.get_value() && active(run)
                                                    })
                                            })
                                }}
                                on:click={move |_| {
                                    force.set(false);
                                    removing.set(Some(id.get_value()));
                                }}
                            >
                                {i18n.text("message.remove_without_forcing")}
                            </button>
                        </article>
                    }
                }}
            />
            <p class="hint" hidden={move || cards.with(|cards| cards.iter().any(cleanup_candidate))}>
                {i18n.text("message.no_folders_eligible_for_cleanup")}
            </p>
        </details>
        <For
            each={move || {
                cards
                    .get()
                    .into_iter()
                    .filter(|card| {
                        card.fields.raw("worktree").is_none()
                            && (card.fields.raw("pullRequest").is_some()
                                || card.fields.raw("connectorPullRequest").is_some())
                    })
                    .collect::<Vec<_>>()
            }}
            key={|card| serde_json::to_string(card).unwrap_or_default()}
            children={move |card| {
                let id = card.id.clone();
                view! {
                    <article class="workspace-row">
                        <div>
                            <h3>{title(&card)}</h3>
                            <PullRequest card />
                        </div>
                        <div class="row-actions">
                            <button data-open-card={id.clone()} on:click={move |_| on_open.run(id.clone())}>
                                {i18n.text("message.open_task")}
                            </button>
                        </div>
                    </article>
                }
            }}
        />
        <For
            each={move || {
                cards
                    .get()
                    .into_iter()
                    .filter(|card| card.fields.raw("worktree").is_some())
                    .collect::<Vec<_>>()
            }}
            key={|card| serde_json::to_string(card).unwrap_or_default()}
            children={move |card| {
                let id = StoredValue::new(card.id.clone());
                let worktree = card.fields.get::<Worktree>("worktree").ok().flatten();
                let key = field(&nested(&card.fields, "ticket"), "key");
                let services = card
                    .fields
                    .get::<atelier_core::application::worktrees::ServiceWorktrees>("serviceWorktrees")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                view! {
                    <article class="workspace-row worktree-row">
                        <div>
                            <span class="section-kicker">
                                {if key.is_empty() { i18n.text("message.local_branch") } else { key }}
                            </span>
                            <h3>{title(&card)}</h3>
                            <code class="branch-label">
                                {worktree
                                    .as_ref()
                                    .map(|worktree| worktree.branch.clone())
                                    .unwrap_or_default()}
                            </code>
                            <p class="path-text resource-location">
                                {worktree.map(|worktree| worktree.path).unwrap_or_default()}
                            </p>
                            {services
                                .into_iter()
                                .map(move |(role, worktree)| {
                                    view! {
                                        <p class="path-text resource-location">
                                            {i18n
                                                .text("message.worktree_0_1_2")
                                                .replace("{0}", &role)
                                                .replace("{1}", &worktree.path)
                                                .replace("{2}", &worktree.branch)}
                                        </p>
                                    }
                                })
                                .collect_view()}
                            <p class="hint">{move || check_label(&id.get_value(), false)}</p>
                            <PullRequest card />
                        </div>
                        <div class="row-actions">
                            <button
                                data-open-card={id.get_value()}
                                disabled={move || busy.get()}
                                on:click={move |_| on_open.run(id.get_value())}
                            >
                                {i18n.text("message.open_task")}
                            </button>
                            <button
                                class="danger"
                                data-clean-worktree={id.get_value()}
                                disabled={move || {
                                    busy.get()
                                        || runs
                                            .with(|runs| {
                                                runs
                                                    .iter()
                                                    .any(|run| {
                                                        field(run, "cardId") == id.get_value() && active(run)
                                                    })
                                            })
                                }}
                                on:click={move |_| {
                                    force.set(true);
                                    removing.set(Some(id.get_value()));
                                }}
                            >
                                {i18n.text("message.remove_worktree_e8412d13")}
                            </button>
                        </div>
                    </article>
                }
            }}
        />
        <p
            class="empty"
            hidden={move || {
                cards.with(|cards| cards.iter().any(|card| card.fields.raw("worktree").is_some()))
            }}
        >
            {i18n.text("message.no_worktrees_the_first_will_be_created_when_a_task_starts")}
        </p>
        <Show when={move || removing.get().is_some()}>
            <RemoveConfirmation busy force removing on_confirm={Callback::new(move |()| remove())} />
        </Show>
    }
}

#[component]
fn RemoveConfirmation(
    busy: RwSignal<bool>,
    force: RwSignal<bool>,
    removing: RwSignal<Option<String>>,
    on_confirm: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |dialog| {
        let _ = dialog.show_modal();
    });
    let cancel = move || {
        if !busy.get_untracked() {
            removing.set(None);
        }
    };
    view! {
        <dialog
            class="confirm-dialog"
            role="alertdialog"
            aria-labelledby="worktree-confirm-title"
            aria-describedby="worktree-confirm-message"
            node_ref={dialog}
            on:cancel={move |event: web_sys::Event| {
                event.prevent_default();
                cancel();
            }}
        >
            <div class="dialog-heading">
                <div class="confirm-icon" aria-hidden="true">
                    <svg
                        width="24"
                        height="24"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M10.3 4.6 2.5 18a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 4.6a2 2 0 0 0-3.4 0Z" />
                        <path d="M12 9v4m0 4h.01" />
                    </svg>
                </div>
                <button
                    type="button"
                    disabled={move || busy.get()}
                    aria-label={move || i18n.text("message.close")}
                    on:click={move |_| cancel()}
                >
                    "×"
                </button>
            </div>
            <h2 id="worktree-confirm-title">
                {move || {
                    i18n
                        .text(
                            if force.get() {
                                "message.remove_worktree_eb977667"
                            } else {
                                "message.clean_up_this_folder"
                            },
                        )
                }}
            </h2>
            <p id="worktree-confirm-message">
                {move || {
                    i18n
                        .text(
                            if force.get() {
                                "message.remove_the_main_worktree_and_associated_front_back_worktrees_del"
                            } else {
                                "message.remove_the_main_and_associated_worktrees_without_forcing_branche"
                            },
                        )
                }}
            </p>
            <div class="form-actions">
                <button type="button" autofocus disabled={move || busy.get()} on:click={move |_| cancel()}>
                    {i18n.text("message.cancel")}
                </button>
                <button
                    class="confirm-action"
                    type="button"
                    disabled={move || busy.get()}
                    on:click={move |_| on_confirm.run(())}
                >
                    {move || {
                        i18n
                            .text(
                                if force.get() {
                                    "message.remove_and_delete_files"
                                } else {
                                    "message.remove_without_forcing"
                                },
                            )
                    }}
                </button>
            </div>
        </dialog>
    }
}
