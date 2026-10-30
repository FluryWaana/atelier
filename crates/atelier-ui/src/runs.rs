use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    contracts::{Worktree, WorktreeState},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

fn active(run: &Document) -> bool {
    matches!(
        field(run, "status").as_str(),
        "starting" | "running" | "stopping"
    )
}

use atelier_core::contracts::RunLog as Log;

#[component]
pub fn Runs(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    reply_drafts: RwSignal<std::collections::BTreeSet<String>>,
    blocked: Memo<bool>,
    on_open: Callback<String>,
    on_applied: Callback<atelier_core::contracts::Card>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card = StoredValue::new(card_id);
    let brainstorm = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .is_some_and(|item| item.fields.flag("brainstormSession").unwrap_or(false))
        })
    });
    let message = RwSignal::new(String::new());
    let message_id = StoredValue::new(format!("brainstorm-message:{}", card.get_value()));
    Effect::new(move |_| {
        reply_drafts.update(|drafts| {
            if !message.get().is_empty() {
                drafts.insert(message_id.get_value());
            } else {
                drafts.remove(&message_id.get_value());
            }
        })
    });
    on_cleanup(move || {
        let _ = reply_drafts.try_update(|drafts| {
            drafts.remove(&message_id.get_value());
        });
    });
    let agent = RwSignal::new(String::new());
    let override_agent = RwSignal::new(false);
    Effect::new(move |_| {
        if !override_agent.get() {
            let saved = board.cards.with(|cards| {
                cards
                    .iter()
                    .find(|item| item.id == card.get_value())
                    .map(|item| field(&item.fields, "agent"))
                    .unwrap_or_default()
            });
            let saved = if saved.is_empty() {
                board.settings.with(|settings| field(settings, "agent"))
            } else {
                saved
            };
            agent.set(saved);
        }
    });
    let mode = RwSignal::new("analyze".to_owned());
    let purpose = RwSignal::new("work".to_owned());
    let error = RwSignal::new(String::new());
    let selected_run = RwSignal::new(String::new());
    let profiles = Signal::derive(move || {
        board.settings.with(|settings| {
            atelier_core::domain::agent_options::providers(settings).unwrap_or_default()
        })
    });
    let history = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .filter(|run| field(run, "cardId") == card.get_value())
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let active_run = Signal::derive(move || history.get().into_iter().find(active));
    let latest = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .find(|run| {
                    field(run, "cardId") == card.get_value()
                        && (selected_run.get().is_empty() || field(run, "id") == selected_run.get())
                })
                .cloned()
        })
    });
    let latest_id = Memo::new(move |_| latest.get().map(|run| field(&run, "id")));
    let running = Memo::new(move |_| active_run.get().is_some());
    let ready = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .is_some_and(|item| {
                    !item.fields.flag("archived").unwrap_or(true)
                        && (((item.fields.flag("brainstormSession").unwrap_or(false)
                            || purpose.get() == "preparation")
                            && field(&item.fields, "repo").is_empty())
                            || item
                                .fields
                                .get::<Worktree>("worktree")
                                .ok()
                                .flatten()
                                .is_some_and(|worktree| worktree.state == WorktreeState::Ready))
                })
        })
    });
    let alive = Rc::new(Cell::new(true));
    let acting = alive.clone();
    let cancelling = RwSignal::new(false);
    let verification_command = Signal::derive(move || {
        let repo = board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .map(|item| field(&item.fields, "repo"))
                .unwrap_or_default()
        });
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        board.projects.with(|projects| {
            projects
                .iter()
                .find(|project| {
                    !repo.is_empty()
                        && atelier_core::domain::projects::same_repository(
                            &field(project, "repo"),
                            &repo,
                            windows,
                        )
                })
                .map(|project| field(project, "testCommand"))
                .unwrap_or_default()
        })
    });
    let workflow_enabled = Memo::new(move |_| {
        let repository = board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .map(|item| field(&item.fields, "repo"))
                .unwrap_or_default()
        });
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        board.projects.with(|projects| {
            projects.iter().any(|project| {
                !repository.is_empty()
                    && atelier_core::domain::projects::same_repository(
                        &field(project, "repo"),
                        &repository,
                        windows,
                    )
                    && project.get::<bool>("automationEnabled").ok().flatten() == Some(true)
            })
        })
    });
    let act = move |action: &str| {
        let pausing = action == "pausequeue";
        let stop = action == "stop" || pausing;
        let verifying = action == "verify";
        let queuing = matches!(action, "queue" | "queue-workflow");
        if cancelling.get_untracked() || (!stop && pending.get_untracked()) {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = if pausing {
            "/api/queue/pause".to_owned()
        } else if stop {
            let Some(run) = active_run.get_untracked() else {
                return;
            };
            format!("/api/runs/{}/stop", field(&run, "id"))
        } else if queuing {
            "/api/queue".to_owned()
        } else if verifying {
            format!("/api/cards/{}/verification/start", card.get_value())
        } else {
            format!("/api/cards/{}/runs/start", card.get_value())
        };
        let body = if stop {
            serde_json::json!({})
        } else if queuing {
            let expected = board.cards.with_untracked(|cards| {
                cards
                    .iter()
                    .find(|item| item.id == card.get_value())
                    .map(atelier_core::application::jira::card_snapshot)
            });
            let revision = board.settings.with_untracked(|settings| {
                settings
                    .get::<atelier_core::contracts::RunQueue>("runQueue")
                    .ok()
                    .flatten()
                    .unwrap_or_default()
                    .revision
            });
            serde_json::json!({"action":"add","revision":revision,"confirmed":true,"expected":expected,"workflow":action=="queue-workflow"})
        } else if verifying {
            let expected = board.cards.with_untracked(|cards| {
                cards
                    .iter()
                    .find(|item| item.id == card.get_value())
                    .cloned()
            });
            serde_json::json!({"confirmed":true,"command":verification_command.get_untracked(),"expected":expected})
        } else {
            let expected = board.cards.with_untracked(|cards| {
                cards
                    .iter()
                    .find(|item| item.id == card.get_value())
                    .cloned()
            });
            serde_json::json!({"agent":agent.get_untracked(),"mode":if brainstorm.get_untracked()||["preparation","review"].contains(&purpose.get_untracked().as_str()){"analyze".to_owned()}else{mode.get_untracked()},"expected":expected,"message":message.get_untracked(),"purpose":purpose.get_untracked()})
        };
        if stop {
            cancelling.set(true);
        } else {
            pending.set(true);
        }
        error.set(String::new());
        let alive = acting.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body).await?;
                board.refresh(&credential).await
            }
            .await;
            if alive.get() {
                if result.is_ok() && !stop {
                    selected_run.set(String::new());
                    message.set(String::new());
                }
                if let Err(message) = result {
                    error.set(message);
                }
                if stop {
                    cancelling.set(false);
                } else {
                    pending.set(false);
                }
            }
        });
    };
    let act = StoredValue::new_local(act);
    let log = RwSignal::new(String::new());
    let cursor = RwSignal::new(0i64);
    let selected = RwSignal::new(String::new());
    let loading = Rc::new(Cell::new(false));
    let log_busy = RwSignal::new(false);
    let refresh = {
        let alive = alive.clone();
        let loading = loading.clone();
        Rc::new(move || {
            let Some(run) = latest.get_untracked() else {
                return;
            };
            let id = field(&run, "id");
            if !alive.get() || loading.replace(true) {
                return;
            }
            log_busy.set(true);
            if selected.get_untracked() != id {
                selected.set(id.clone());
                cursor.set(0);
                log.set(String::new());
            }
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    loading.set(false);
                    log_busy.set(false);
                    error.set(message);
                    return;
                }
            };
            let after = cursor.get_untracked();
            let alive = alive.clone();
            let loading = loading.clone();
            spawn_local(async move {
                let result = async {
                    read(&credential, &format!("/api/runs/{id}/logs?after={after}"))
                        .await?
                        .json::<Vec<Log>>()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())
                }
                .await;
                if alive.get() && selected.get_untracked() == id {
                    match result {
                        Ok(entries) => {
                            for entry in entries {
                                cursor.set(entry.seq);
                                log.update(|text| {
                                    text.push_str(&format!("[{}] {}", entry.stream, entry.text));
                                    if !text.ends_with('\n') {
                                        text.push('\n');
                                    }
                                    if text.len() > 64 * 1024 {
                                        let mut start = text.len() - 64 * 1024;
                                        while !text.is_char_boundary(start) {
                                            start += 1;
                                        }
                                        text.drain(..start);
                                    }
                                });
                            }
                        }
                        Err(message) => error.set(message),
                    }
                }
                loading.set(false);
                if alive.get() {
                    log_busy.set(false);
                }
            });
        })
    };
    let initial = refresh.clone();
    Effect::new(move |_| {
        let current = latest_id.get().unwrap_or_default();
        selected.set(current);
        cursor.set(0);
        log.set(String::new());
        initial();
    });
    let tick = Closure::<dyn FnMut()>::new(move || refresh());
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((alive, interval, tick));
    on_cleanup(move || {
        timer.with_value(|(alive, interval, _)| {
            alive.set(false);
            if let (Some(window), Some(interval)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(interval);
            }
        })
    });
    view! {
        <section
            class="task-files"
            aria-labelledby="runs-heading"
            aria-busy={move || (pending.get() || cancelling.get()).to_string()}
        >
            <h3 id="runs-heading">{move || i18n.text("message.runs")}</h3>
            <p>
                {move || {
                    latest
                        .get()
                        .and_then(|run| run.get::<serde_json::Value>("usage").ok().flatten())
                        .and_then(|usage| {
                            let used = usage["contextUsedTokens"].as_u64()?;
                            let size = usage["contextWindowTokens"].as_u64()?;
                            (size >= used)
                                .then(|| {
                                    i18n
                                        .text(
                                            "message.context_0_tokens_remaining_out_of_1_this_is_not_the_account_quot",
                                        )
                                        .replace("{0}", &(size - used).to_string())
                                        .replace("{1}", &size.to_string())
                                })
                        })
                        .unwrap_or_default()
                }}
            </p>
            <p class="field-help">
                {move || {
                    i18n.text("message.choose_a_profile_saved_in_agents_approval_requests_and_questions")
                }}
            </p>
            <label for="run-agent">{move || i18n.text("message.agent_profile")}</label>
            <p class="field-help" hidden={move || !profiles.get().is_empty()}>
                {move || i18n.text("message.create_a_command_profile_in_agents_to_start_a_run")}
            </p>
            <select
                id="run-agent"
                prop:value={move || agent.get()}
                disabled={move || pending.get() || running.get()}
                on:change={move |event| {
                    override_agent.set(true);
                    agent.set(event_target_value(&event));
                }}
            >
                <option value="">{move || i18n.text("message.choose_a_profile")}</option>
                <For
                    each={move || profiles.get()}
                    key={|profile| field(profile, "id")}
                    children={move |profile| {
                        view! { <option value={field(&profile, "id")}>{field(&profile, "name")}</option> }
                    }}
                />
            </select>
            <div hidden={move || brainstorm.get()}>
                <label for="run-purpose">{move || i18n.text("message.run_purpose")}</label>
                <select
                    id="run-purpose"
                    prop:value={move || purpose.get()}
                    disabled={move || pending.get() || running.get()}
                    on:change={move |event| purpose.set(event_target_value(&event))}
                >
                    <option value="work">{move || i18n.text("message.work_on_the_task")}</option>
                    <option value="preparation">
                        {move || i18n.text("message.prepare_criteria_and_questions")}
                    </option>
                    <option value="review">
                        {move || i18n.text("message.review_the_pr_mr_and_its_criteria")}
                    </option>
                </select>
            </div>
            <label
                hidden={move || {
                    brainstorm.get() || ["preparation", "review"].contains(&purpose.get().as_str())
                }}
                for="run-mode"
            >
                {move || i18n.text("message.mode")}
            </label>
            <select
                hidden={move || {
                    brainstorm.get() || ["preparation", "review"].contains(&purpose.get().as_str())
                }}
                id="run-mode"
                prop:value={move || mode.get()}
                disabled={move || pending.get() || running.get()}
                on:change={move |event| mode.set(event_target_value(&event))}
            >
                <option value="analyze">{move || i18n.text("message.analysis")}</option>
                <option value="edit">{move || i18n.text("message.editing")}</option>
            </select>
            <div hidden={move || !brainstorm.get()}>
                <label for="brainstorm-message">
                    {move || i18n.text("message.message_for_the_brainstorm")}
                </label>
                <textarea
                    id="brainstorm-message"
                    maxlength="12000"
                    rows="4"
                    disabled={move || pending.get() || running.get()}
                    prop:value={move || message.get()}
                    on:input={move |event| message.set(event_target_value(&event))}
                />
                <p class="field-help">
                    {move || {
                        i18n.text("message.describe_your_question_or_request_a_task_breakdown_each_message")
                    }}
                </p>
            </div>
            <div class="editor-actions">
                <button
                    id="start-run"
                    type="button"
                    disabled={move || {
                        pending.get() || cancelling.get() || blocked.get() || running.get() || !ready.get()
                            || agent.get().is_empty() || (brainstorm.get() && message.get().trim().is_empty())
                    }}
                    on:click={move |_| act.with_value(|act| act("start"))}
                >
                    {move || i18n.text("message.start_run")}
                </button>
                <button
                    id="stop-run"
                    type="button"
                    disabled={move || cancelling.get() || !running.get()}
                    on:click={move |_| act.with_value(|act| act("stop"))}
                >
                    {move || i18n.text("message.stop_run")}
                </button>
            </div>
            <p
                class="field-help"
                hidden={move || {
                    !brainstorm.get()
                        || board
                            .cards
                            .with(|cards| {
                                cards
                                    .iter()
                                    .find(|item| item.id == card.get_value())
                                    .is_none_or(|item| !field(&item.fields, "repo").is_empty())
                            })
                }}
            >
                {move || {
                    i18n.text("message.conversation_without_a_repository_each_exchange_uses_an_empty_te")
                }}
            </p>
            <p class="field-help" hidden={move || ready.get()}>
                {move || {
                    i18n.text("message.prepare_and_verify_the_worktree_then_save_your_changes_before_st")
                }}
            </p>
            <div hidden={move || brainstorm.get()}>
                <button
                    id="enqueue-task"
                    type="button"
                    disabled={move || pending.get() || blocked.get() || running.get() || !ready.get()}
                    on:click={move |_| act.with_value(|act| act("queue"))}
                >
                    {move || i18n.text("message.add_this_task_to_the_run_queue")}
                </button>
                <button
                    id="enqueue-workflow"
                    type="button"
                    hidden={move || !workflow_enabled.get()}
                    disabled={move || pending.get() || blocked.get() || running.get() || !ready.get()}
                    on:click={move |_| act.with_value(|act| act("queue-workflow"))}
                >
                    {move || i18n.text("message.add_the_project_workflow_to_the_queue")}
                </button>
                <p class="field-help">
                    {move || {
                        i18n.text("message.the_queue_uses_the_task_s_saved_settings_not_the_temporary_selec")
                    }}
                </p>
            </div>
            <button
                id="pause-task-queue"
                type="button"
                hidden={move || {
                    board
                        .settings
                        .with(|settings| {
                            settings
                                .get::<atelier_core::contracts::RunQueue>("runQueue")
                                .ok()
                                .flatten()
                                .unwrap_or_default()
                                .paused
                        })
                }}
                disabled={move || cancelling.get()}
                on:click={move |_| act.with_value(|act| act("pausequeue"))}
            >
                {move || i18n.text("message.pause_queue")}
            </button>
            <div hidden={move || brainstorm.get()}>
                <h4>{move || i18n.text("message.source_verification")}</h4>
                <p class="field-help" hidden={move || !verification_command.get().is_empty()}>
                    {move || i18n.text("message.configure_the_verification_command_in_the_project")}
                </p>
                <pre class="service-log" tabindex="0">
                    {move || verification_command.get()}
                </pre>
                <button
                    id="start-verification"
                    type="button"
                    disabled={move || {
                        pending.get() || blocked.get() || running.get() || !ready.get()
                            || verification_command.get().trim().is_empty()
                    }}
                    on:click={move |_| act.with_value(|act| act("verify"))}
                >
                    {move || i18n.text("message.confirm_and_run_verification")}
                </button>
                <p class="field-help">
                    {move || {
                        i18n.text("message.the_command_uses_your_account_permissions_without_an_agent_or_va")
                    }}
                </p>
            </div>
            <p
                id="verification-result"
                role="status"
                hidden={move || latest.get().is_none_or(|run| field(&run, "purpose") != "verification")}
            >
                {move || {
                    latest
                        .get()
                        .map(|run| {
                            i18n
                                .text(
                                    match field(&run, "verificationState").as_str() {
                                        "passed" => "message.verification_passed_for_this_source_snapshot",
                                        "failed" => "message.verification_failed_or_stopped",
                                        "stale" => {
                                            "message.stale_result_sources_requirements_or_command_changed"
                                        }
                                        "interrupted" => {
                                            "message.verification_was_interrupted_run_the_command_explicitly_again"
                                        }
                                        _ => "message.verification_running",
                                    },
                                )
                        })
                        .unwrap_or_default()
                }}
            </p>
            <p role="status">
                {move || {
                    i18n
                        .text(
                            latest
                                .get()
                                .map(|run| match field(&run, "status").as_str() {
                                    "starting" => "message.starting",
                                    "running" => "message.process_running",
                                    "stopping" => "message.stopping",
                                    "completed" => "message.run_completed",
                                    "cancelled" => "message.run_stopped",
                                    "interrupted" => {
                                        "message.run_interrupted_check_the_repository_before_starting_again"
                                    }
                                    _ => "message.run_failed",
                                })
                                .unwrap_or("message.no_runs_for_this_task"),
                        )
                }}
            </p>
            <p class="error" role="alert">
                {move || {
                    let message = error.get();
                    i18n.text(
                        &if message.is_empty() {
                            latest.get().map(|run| field(&run, "attention")).unwrap_or_default()
                        } else {
                            message
                        },
                    )
                }}
            </p>
            <label for="run-history">{move || i18n.text("message.run_history")}</label>
            <select
                id="run-history"
                prop:value={move || selected_run.get()}
                on:change={move |event| selected_run.set(event_target_value(&event))}
            >
                <option value="">{move || i18n.text("message.latest_run")}</option>
                <For
                    each={move || history.get()}
                    key={|run| field(run, "id")}
                    children={move |run| {
                        view! {
                            <option value={field(
                                &run,
                                "id",
                            )}>{format!("{} · {}", field(&run, "startedAt"), field(&run, "agent"))}</option>
                        }
                    }}
                />
            </select>
            <p
                role="status"
                hidden={move || {
                    !active_run
                        .get()
                        .is_some_and(|run| run.get::<bool>("waiting").ok().flatten().unwrap_or(false))
                }}
            >
                {move || i18n.text("message.waiting_for_your_reply")}
            </p>
            <For
                each={move || {
                    active_run
                        .get()
                        .map(|run| {
                            let id = field(&run, "id");
                            run.get::<Vec<atelier_core::contracts::AgentInteraction>>("interactions")
                                .ok()
                                .flatten()
                                .unwrap_or_default()
                                .into_iter()
                                .map(|request| (id.clone(), request))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                }}
                key={|(id, request)| (
                    id.clone(),
                    request.id.clone(),
                    serde_json::to_string(request).unwrap_or_default(),
                )}
                children={move |(run_id, request)| {
                    view! { <Interaction run_id request token board reply_drafts /> }
                }}
            />
            <pre
                class="service-log"
                tabindex="0"
                hidden={move || active_run.get().is_none_or(|run| field(&run, "liveText").is_empty())}
            >
                {move || active_run.get().map(|run| field(&run, "liveText")).unwrap_or_default()}
            </pre>
            <crate::review::ReviewResult
                card_id={card.get_value()}
                board
                token
                pending
                blocked
                run={latest}
                agent
                reply_drafts
            />
            <Show when={move || !brainstorm.get()}>
                <crate::preparation::PreparationResult
                    card_id={card.get_value()}
                    board
                    token
                    pending
                    blocked
                    run={latest}
                    on_applied
                />
            </Show>
            <Show when={move || brainstorm.get()}>
                <crate::brainstorm::Results
                    card_id={card.get_value()}
                    board
                    token
                    pending
                    blocked
                    message
                    run={latest}
                    on_open
                />
            </Show>
            <details hidden={move || latest.get().is_none_or(|run| field(&run, "summary").is_empty())}>
                <summary>{move || i18n.text("message.run_summary")}</summary>
                <p>{move || latest.get().map(|run| field(&run, "summary")).unwrap_or_default()}</p>
            </details>
            <p hidden={move || {
                latest.get().is_none_or(|run| run.raw("usage").is_none())
            }}>
                {move || {
                    latest
                        .get()
                        .and_then(|run| run.get::<serde_json::Value>("usage").ok().flatten())
                        .map(|usage| {
                            format!(
                                "{} {} / {} / {}",
                                i18n.text("message.input_cache_output_tokens"),
                                usage["inputTokens"]
                                    .as_u64()
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "-".into()),
                                usage["cachedInputTokens"]
                                    .as_u64()
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "-".into()),
                                usage["outputTokens"]
                                    .as_u64()
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "-".into()),
                            )
                        })
                        .unwrap_or_default()
                }}
            </p>
            <p>
                {move || {
                    latest
                        .get()
                        .and_then(|run| run.get::<serde_json::Value>("usage").ok().flatten())
                        .filter(|usage| {
                            usage["costSource"] == "claude-cli" && usage["costScope"] == "cli-session"
                        })
                        .and_then(|usage| usage["estimatedCostUsd"].as_f64())
                        .filter(|amount| amount.is_finite() && (0.0..=1_000_000.0).contains(amount))
                        .map(|amount| {
                            i18n
                                .text(
                                    "message.estimate_reported_by_claude_0_usd_may_include_a_resumed_session",
                                )
                                .replace("{0}", &format!("{amount:.6}"))
                        })
                        .unwrap_or_default()
                }}
            </p>
            <p>
                {move || {
                    let Some(run) = latest.get().filter(|run| field(run, "agentType") == "codex") else {
                        return String::new();
                    };
                    let Some(usage) = run.get::<serde_json::Value>("usage").ok().flatten() else {
                        return String::new();
                    };
                    let amount = usage["estimatedCostUsd"]
                        .as_f64()
                        .filter(|amount| amount.is_finite() && (0.0..=1_000_000.0).contains(amount));
                    if usage["costSource"] == "openai-standard-api"
                        && usage["costScope"] == "requested-model-thread" && let Some(amount) = amount
                    {
                        i18n.text("message.estimated_standard_api_equivalent_usd_0_requested_model_1_prices")
                            .replace("{0}", &format!("{amount:.6}"))
                            .replace("{1}", usage["pricingModel"].as_str().unwrap_or("-"))
                            .replace("{2}", usage["pricingDate"].as_str().unwrap_or("-"))
                    } else {
                        i18n.text("message.codex_estimate_unavailable_for_this_model_or_these_counters_or_a")
                    }
                }}
            </p>
            <p hidden={move || {
                latest.get().is_none_or(|run| run.raw("activeMilliseconds").is_none())
            }}>
                {move || {
                    latest
                        .get()
                        .and_then(|run| run.get::<u64>("activeMilliseconds").ok().flatten())
                        .map(|milliseconds| {
                            format!("{} {} s", i18n.text("message.active_duration"), milliseconds / 1000)
                        })
                        .unwrap_or_default()
                }}
            </p>
            <details>
                <summary>{move || i18n.text("message.latest_run_log")}</summary>
                <p role="status" hidden={move || !log_busy.get() || !log.get().is_empty()}>
                    {move || i18n.text("message.loading_log")}
                </p>
                <pre class="service-log" tabindex="0" aria-busy={move || log_busy.get().to_string()}>
                    <code>{move || log.get()}</code>
                </pre>
            </details>
            <p class="field-help">
                {move || {
                    i18n.text("message.showing_the_last_64_kib_commands_use_their_own_permissions_analy")
                }}
            </p>
        </section>
    }
}

#[component]
pub(super) fn Interaction(
    run_id: String,
    request: atelier_core::contracts::AgentInteraction,
    token: StoredValue<Result<String, String>>,
    board: BoardState,
    reply_drafts: RwSignal<std::collections::BTreeSet<String>>,
) -> impl IntoView {
    use std::collections::BTreeMap;
    let i18n = expect_context::<I18n>();
    let request = StoredValue::new(request);
    let title = move || {
        request.with_value(|request| {
            request
                .title_key
                .as_deref()
                .map(|key| i18n.text(key))
                .unwrap_or_else(|| request.title.clone())
        })
    };
    let run = StoredValue::new(run_id);
    let drafts = RwSignal::new(BTreeMap::<String, String>::new());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let identity = StoredValue::new(format!("{}:{}", run.get_value(), request.get_value().id));
    Effect::new(move |_| {
        let dirty =
            pending.get() || drafts.with(|drafts| drafts.values().any(|value| !value.is_empty()));
        reply_drafts.update(|all| {
            if dirty {
                all.insert(identity.get_value());
            } else {
                all.remove(&identity.get_value());
            }
        });
    });
    on_cleanup(move || {
        let _ = reply_drafts.try_update(|all| {
            all.remove(&identity.get_value());
        });
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let send = move |deny: bool| {
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let request = request.get_value();
        let answers = if deny {
            BTreeMap::new()
        } else {
            request
                .questions
                .iter()
                .map(|question| {
                    let value = drafts.with_untracked(|drafts| {
                        drafts.get(&question.id).cloned().unwrap_or_default()
                    });
                    (
                        question.id.clone(),
                        if question.multi_select {
                            value.lines().map(str::to_owned).collect()
                        } else {
                            vec![value]
                        },
                    )
                })
                .collect()
        };
        let body = match serde_json::to_value(atelier_core::contracts::AgentAnswer {
            request_id: request.id,
            decision: if deny {
                "deny"
            } else if request.questions.is_empty() {
                "allow"
            } else {
                "answer"
            }
            .into(),
            answers,
        }) {
            Ok(body) => body,
            Err(_) => {
                error.set("message.invalid_interactive_reply".into());
                return;
            }
        };
        let path = format!("/api/runs/{}/reply", run.get_value());
        let alive = alive.clone();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body).await?;
                if alive.get() {
                    drafts.set(BTreeMap::new());
                }
                board.refresh(&credential).await
            }
            .await;
            if alive.get() {
                pending.set(false);
                if let Err(message) = result {
                    error.set(message);
                }
            }
        });
    };
    let send = StoredValue::new_local(send);
    view! {
        <form
            class="service-state agent-interaction"
            aria-label={title}
            aria-busy={move || pending.get().to_string()}
            on:submit={move |event| {
                event.prevent_default();
                send.with_value(|send| send(false));
            }}
        >
            <h4>{title}</h4>
            <pre class="service-log" tabindex="0" hidden={move || request.get_value().detail.is_empty()}>
                {move || request.get_value().detail}
            </pre>
            <p class="field-help" hidden={move || !request.get_value().secret_warning}>
                {move || i18n.text("message.agent_form_secret_warning")}
            </p>
            <For
                each={move || request.get_value().questions}
                key={|question| question.id.clone()}
                children={move |question| {
                    let question = StoredValue::new(question);
                    let input_id = StoredValue::new(
                        format!("{}-{}-{}", run.get_value(), request.get_value().id, question.get_value().id),
                    );
                    view! {
                        <label for={move || {
                            input_id.get_value()
                        }}>{move || question.get_value().question}</label>
                        <ul>
                            <For
                                each={move || question.get_value().options}
                                key={|option| option.clone()}
                                children={move |option| {
                                    let option = StoredValue::new(option);
                                    view! {
                                        <li>
                                            <button
                                                type="button"
                                                disabled={move || pending.get()}
                                                on:click={move |_| {
                                                    drafts
                                                        .update(|drafts| {
                                                            let value = drafts
                                                                .entry(question.get_value().id)
                                                                .or_default();
                                                            if question.get_value().multi_select && !value.is_empty() {
                                                                value.push('\n');
                                                                value.push_str(&option.get_value());
                                                            } else {
                                                                *value = option.get_value();
                                                            }
                                                        })
                                                }}
                                            >
                                                {move || option.get_value()}
                                            </button>
                                        </li>
                                    }
                                }}
                            />
                        </ul>
                        {if question.get_value().multi_select && !question.get_value().is_secret {
                            view! {
                                <p class="field-help">
                                    {move || {
                                        i18n
                                            .text(
                                                "message.one_answer_per_line_you_can_also_enter_another_choice",
                                            )
                                    }}
                                </p>
                                <textarea
                                    id={move || input_id.get_value()}
                                    required={move || question.get_value().required}
                                    maxlength="32000"
                                    disabled={move || pending.get()}
                                    prop:value={move || {
                                        drafts
                                            .with(|drafts| {
                                                drafts
                                                    .get(&question.get_value().id)
                                                    .cloned()
                                                    .unwrap_or_default()
                                            })
                                    }}
                                    on:input={move |event| {
                                        drafts
                                            .update(|drafts| {
                                                drafts
                                                    .insert(
                                                        question.get_value().id,
                                                        event_target_value(&event),
                                                    );
                                            })
                                    }}
                                />
                            }
                                .into_any()
                        } else {
                            view! {
                                <input
                                    class="agent-question"
                                    id={move || input_id.get_value()}
                                    type={if question.get_value().is_secret { "password" } else { "text" }}
                                    autocomplete="off"
                                    required={move || question.get_value().required}
                                    maxlength="4000"
                                    disabled={move || pending.get()}
                                    prop:value={move || {
                                        drafts
                                            .with(|drafts| {
                                                drafts
                                                    .get(&question.get_value().id)
                                                    .cloned()
                                                    .unwrap_or_default()
                                            })
                                    }}
                                    on:input={move |event| {
                                        drafts
                                            .update(|drafts| {
                                                drafts
                                                    .insert(
                                                        question.get_value().id,
                                                        event_target_value(&event),
                                                    );
                                            })
                                    }}
                                />
                            }
                                .into_any()
                        }}
                    }
                }}
            />
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <div class="editor-actions">
                <button
                    class="approve-interaction"
                    type="submit"
                    disabled={move || {
                        pending.get()
                            || (!request.get_value().can_allow && request.get_value().questions.is_empty())
                    }}
                >
                    {move || {
                        i18n
                            .text(
                                if request.get_value().questions.is_empty() {
                                    "message.approve_this_request"
                                } else {
                                    "message.send_reply"
                                },
                            )
                    }}
                </button>
                <button
                    class="deny-interaction"
                    type="button"
                    disabled={move || pending.get()}
                    on:click={move |_| send.with_value(|send| send(true))}
                >
                    {move || i18n.text("message.deny_this_request")}
                </button>
            </div>
        </form>
    }
}
