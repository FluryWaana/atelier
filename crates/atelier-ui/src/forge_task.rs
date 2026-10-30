use crate::{i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeDraftIntent, ForgeDraftRequest, ForgeKind, ForgeWriteState,
    PullRequest, PullRequestAttach, PullRequestDetach,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
type Draft = (ForgeKind, String, String);
#[component]
pub fn ForgeTask(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    archived: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let linked = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<PullRequest>("pullRequest").ok().flatten())
    });
    let initial = linked
        .get_untracked()
        .map(|pr| (pr.kind, pr.repository, pr.number.to_string()))
        .unwrap_or((ForgeKind::Github, String::new(), String::new()));
    let original = RwSignal::new(initial.clone());
    let draft = RwSignal::new(initial);
    let configuration = RwSignal::new(None::<ForgeConnections>);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let approved = RwSignal::new(None::<(Draft, u64)>);
    let detach_ack = RwSignal::new(String::new());
    let draft_ack = RwSignal::new(String::new());
    let publication_dirty = RwSignal::new(false);
    let content_dirty = RwSignal::new(false);
    Effect::new(move |_| {
        let next = linked
            .get()
            .map(|pr| (pr.kind, pr.repository, pr.number.to_string()))
            .unwrap_or((ForgeKind::Github, String::new(), String::new()));
        if draft.get_untracked() == original.get_untracked() && original.get_untracked() != next {
            original.set(next.clone());
            draft.set(next);
            approved.set(None);
            detach_ack.set(String::new());
            draft_ack.set(String::new());
        }
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgeDraftIntent>("forgeDraft")
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
    let draft_key = Signal::derive(move || {
        let card = card.get()?;
        let configuration = configuration.get()?;
        let linked = linked.get()?;
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card),"revision":configuration.revision,"draft":if uncertain.get(){intent.get()?.draft}else{!linked.draft}}).to_string())
    });
    Effect::new(move |_| {
        dirty.set(draft.get() != original.get() || publication_dirty.get() || content_dirty.get())
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let reading = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        loading.set(true);
        error.set(String::new());
        approved.set(None);
        let alive = reading.clone();
        spawn_local(async move {
            let result = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(value) => configuration.set(Some(value)),
                    Err(message) => error.set(message),
                }
            }
        });
    });
    Effect::new(move |_| load.with_value(|load| load()));
    let writing = alive.clone();
    let change_draft = move |reconcile: bool| {
        if pending.get_untracked()
            || loading.get_untracked()
            || (!reconcile
                && (publication_dirty.get_untracked()
                    || content_dirty.get_untracked()
                    || blocked.get_untracked()
                    || draft.get_untracked() != original.get_untracked()))
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        let Some(pr) = linked.get_untracked() else {
            return;
        };
        if reconcile != uncertain.get_untracked()
            || (!reconcile
                && (archived
                    || unresolved.get_untracked()
                    || draft_key.get_untracked().as_deref()
                        != Some(draft_ack.get_untracked().as_str())))
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
        let target = if reconcile {
            let Some(intent) = intent.get_untracked() else {
                return;
            };
            intent.draft
        } else {
            !pr.draft
        };
        let body = serde_json::to_value(ForgeDraftRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            draft: target,
            confirmed: true,
            content: if reconcile {
                intent.get_untracked().and_then(|intent| intent.content)
            } else {
                None
            },
        })
        .expect("forge draft serialization");
        let path = format!(
            "/api/cards/{}/forge/{}",
            id.get_value(),
            if reconcile {
                "draft-reconcile"
            } else {
                "draft"
            }
        );
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        draft_ack.set(String::new());
        let alive = writing.clone();
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
                        let still_uncertain =
                            atelier_core::application::forge::unresolved(&card.fields)
                                .unwrap_or(true);
                        replace_card(board.cards, card);
                        notice.set(if still_uncertain{"message.requested_state_not_observed_the_write_remains_uncertain_no_auto"}else{"message.draft_state_observed_on_the_forge_revalidate_reviews_before_cont"}.into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    let change_draft = StoredValue::new_local(change_draft);
    let execute = StoredValue::new_local(move |remove: bool| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || publication_dirty.get_untracked()
            || content_dirty.get_untracked()
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
        let body = if remove {
            let Some(linked) = linked.get_untracked() else {
                return;
            };
            if detach_ack.get_untracked() != serde_json::to_string(&linked).unwrap_or_default() {
                return;
            }
            serde_json::to_value(PullRequestDetach {
                expected: atelier_core::application::jira::card_snapshot(&expected),
                confirmed: true,
            })
            .expect("PR detach serialization")
        } else {
            if loading.get_untracked() || archived {
                return;
            }
            let Some(configuration) = configuration.get_untracked() else {
                return;
            };
            let draft = draft.get_untracked();
            if approved.get_untracked() != Some((draft.clone(), configuration.revision)) {
                return;
            }
            let Some(number) = draft
                .2
                .parse::<u64>()
                .ok()
                .filter(|number| *number > 0 && *number <= i32::MAX as u64)
            else {
                error.set("message.a_positive_integer_pr_mr_number_is_required".into());
                return;
            };
            serde_json::to_value(PullRequestAttach {
                expected: atelier_core::application::jira::card_snapshot(&expected),
                revision: configuration.revision,
                kind: draft.0,
                repository: draft.1,
                number,
                confirmed: true,
            })
            .expect("PR attach serialization")
        };
        let path = format!(
            "/api/cards/{}/forge/{}",
            id.get_value(),
            if remove { "detach" } else { "attach" }
        );
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
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
                        let next = card
                            .fields
                            .get::<PullRequest>("pullRequest")
                            .ok()
                            .flatten()
                            .map(|pr| (pr.kind, pr.repository, pr.number.to_string()))
                            .unwrap_or((ForgeKind::Github, String::new(), String::new()));
                        original.set(next.clone());
                        draft.set(next);
                        replace_card(board.cards, card);
                        approved.set(None);
                        detach_ack.set(String::new());
                        notice.set(if remove{"message.link_removed_no_remote_pr_mr_changed"}else{"message.pr_mr_linked_after_repository_and_branch_verification_no_remote"}.into());
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
    let brain = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref()
                .is_some_and(|card| card.fields.flag("brainstormSession").unwrap_or(false))
        })
    });
    view! {
        <section
            class="task-files"
            hidden={move || brain.get()}
            aria-labelledby="forge-task-heading"
            aria-busy={move || (pending.get() || loading.get()).to_string()}
        >
            <h3 id="forge-task-heading">{move || i18n.text("message.task_pr_mr")}</h3>
            {move || {
                linked
                    .get()
                    .map(|pr| {
                        view! {
                            <p>
                                {
                                    let label = format!("{} #{} · {}", pr.repository, pr.number, pr.title);
                                    if pr.url.starts_with("https://") && !pr.url.chars().any(char::is_control)
                                    {
                                        view! {
                                            <a href={pr.url} target="_blank" rel="noopener noreferrer">
                                                {label}
                                            </a>
                                        }
                                            .into_any()
                                    } else {
                                        view! { <span>{label}</span> }.into_any()
                                    }
                                }
                            </p>
                            <p>{pr.source_branch}{i18n.text("message.to_separator")}{pr.target_branch}</p>
                            <p>{move || i18n.text("message.last_read_0").replace("{0}", &pr.checked_at)}</p>
                        }
                    })
            }}
            <p class="field-help">
                {move || {
                    i18n.text("message.link_an_existing_pr_mr_whose_origin_repository_and_source_branch")
                }}
            </p>
            <fieldset disabled={move || {
                pending.get() || loading.get() || blocked.get() || archived || unresolved.get()
                    || publication_dirty.get() || content_dirty.get()
            }}>
                <crate::form::Field
                    id="task-forge-kind"
                    label={Signal::derive(move || i18n.text("message.forge"))}
                >
                    <crate::form::Select
                        {..}
                        id="task-forge-kind"
                        prop:value={move || draft.get().0.name()}
                        on:change={move |event| {
                            draft
                                .update(|draft| {
                                    draft.0 = match event_target_value(&event).as_str() {
                                        "gitlab" => ForgeKind::Gitlab,
                                        "azure" => ForgeKind::Azure,
                                        _ => ForgeKind::Github,
                                    };
                                });
                            approved.set(None);
                        }}
                    >
                        <option value="github">"GitHub"</option>
                        <option value="gitlab">"GitLab"</option>
                        <option value="azure">"Azure DevOps Services"</option>
                    </crate::form::Select>
                </crate::form::Field>
                <crate::form::Field
                    id="task-forge-repository"
                    label={Signal::derive(move || i18n.text("message.pr_mr_repository"))}
                >
                    <crate::form::Input
                        {..}
                        id="task-forge-repository"
                        maxlength="1000"
                        prop:value={move || draft.get().1}
                        on:input={move |event| {
                            draft.update(|draft| draft.1 = event_target_value(&event));
                            approved.set(None);
                        }}
                    />
                </crate::form::Field>
                <crate::form::Field
                    id="task-forge-number"
                    label={Signal::derive(move || i18n.text("message.pr_mr_number"))}
                >
                    <crate::form::Input
                        {..}
                        id="task-forge-number"
                        type="number"
                        min="1"
                        max="2147483647"
                        step="1"
                        prop:value={move || draft.get().2}
                        on:input={move |event| {
                            draft.update(|draft| draft.2 = event_target_value(&event));
                            approved.set(None);
                        }}
                    />
                </crate::form::Field>
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-task-forge"
                        prop:checked={move || {
                            configuration
                                .with(|configuration| {
                                    configuration
                                        .as_ref()
                                        .is_some_and(|configuration| {
                                            approved.get() == Some((draft.get(), configuration.revision))
                                        })
                                })
                        }}
                        on:change={move |event| {
                            approved
                                .set(
                                    if event_target_checked(&event) {
                                        configuration
                                            .with(|configuration| {
                                                configuration
                                                    .as_ref()
                                                    .map(|configuration| (
                                                        draft.get_untracked(),
                                                        configuration.revision,
                                                    ))
                                            })
                                    } else {
                                        None
                                    },
                                )
                        }}
                    />
                    {move || {
                        i18n.text("message.verify_and_link_this_pr_mr_without_changing_anything_on_the_forg")
                    }}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        configuration
                            .with(|configuration| {
                                configuration
                                    .as_ref()
                                    .is_none_or(|configuration| {
                                        approved.get() != Some((draft.get(), configuration.revision))
                                    })
                            })
                    })}
                    {..}
                    id="attach-task-forge"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute(false))}
                >
                    {move || i18n.text("message.verify_and_save_the_link")}
                </crate::button::Button>
                <crate::button::Button
                    {..}
                    id="cancel-task-forge"
                    type="button"
                    on:click={move |_| {
                        draft.set(original.get_untracked());
                        approved.set(None);
                    }}
                >
                    {move || i18n.text("message.cancel_preparation")}
                </crate::button::Button>
                <crate::button::Button
                    {..}
                    id="reload-task-forges"
                    type="button"
                    on:click={move |_| load.with_value(|load| load())}
                >
                    {move || i18n.text("message.reload_forge_connections")}
                </crate::button::Button>
            </fieldset>
            <fieldset
                hidden={move || linked.with(|linked| linked.is_none())}
                disabled={move || {
                    pending.get() || blocked.get() || draft.get() != original.get() || archived
                        || unresolved.get() || publication_dirty.get() || content_dirty.get()
                }}
            >
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-detach-forge"
                        prop:checked={move || {
                            linked
                                .with(|linked| {
                                    linked
                                        .as_ref()
                                        .is_some_and(|linked| {
                                            serde_json::to_string(linked).unwrap_or_default()
                                                == detach_ack.get()
                                        })
                                })
                        }}
                        on:change={move |event| {
                            detach_ack
                                .set(
                                    if event_target_checked(&event) {
                                        linked
                                            .with(|linked| {
                                                linked
                                                    .as_ref()
                                                    .map(|linked| {
                                                        serde_json::to_string(linked).unwrap_or_default()
                                                    })
                                                    .unwrap_or_default()
                                            })
                                    } else {
                                        String::new()
                                    },
                                )
                        }}
                    />
                    {move || i18n.text("message.remove_only_the_local_link_keep_the_pr_mr_and_worktree")}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        linked
                            .with(|linked| {
                                linked
                                    .as_ref()
                                    .is_none_or(|linked| {
                                        serde_json::to_string(linked).unwrap_or_default() != detach_ack.get()
                                    })
                            })
                    })}
                    {..}
                    id="detach-task-forge"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute(true))}
                >
                    {move || i18n.text("message.confirm_removing_the_link")}
                </crate::button::Button>
            </fieldset>
            <fieldset
                hidden={move || {
                    linked.with(|linked| linked.as_ref().is_none_or(|pr| pr.status != "active"))
                        || unresolved.get()
                }}
                disabled={move || {
                    pending.get() || loading.get() || blocked.get() || archived || publication_dirty.get()
                        || content_dirty.get() || draft.get() != original.get()
                        || configuration.with(|value| value.is_none())
                }}
            >
                <p class="field-help">
                    {move || {
                        i18n.text("message.this_writes_to_the_forge_marking_ready_for_review_may_notify_rev")
                    }}
                </p>
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-forge-draft"
                        prop:checked={move || draft_key.get().is_some_and(|key| key == draft_ack.get())}
                        on:change={move |event| {
                            draft_ack
                                .set(
                                    if event_target_checked(&event) {
                                        draft_key.get_untracked().unwrap_or_default()
                                    } else {
                                        String::new()
                                    },
                                )
                        }}
                    />
                    {move || {
                        i18n.text(
                            if linked.get().is_some_and(|pr| pr.draft) {
                                "message.confirm_marking_this_pr_mr_ready_for_review"
                            } else {
                                "message.confirm_converting_this_pr_mr_to_a_draft"
                            },
                        )
                    }}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || draft_key.get().is_none_or(|key| key != draft_ack.get()))}
                    {..}
                    id="change-forge-draft"
                    type="button"
                    on:click={move |_| change_draft.with_value(|change| change(false))}
                >
                    {move || {
                        i18n.text(
                            if linked.get().is_some_and(|pr| pr.draft) {
                                "message.mark_ready_for_review"
                            } else {
                                "message.convert_to_draft"
                            },
                        )
                    }}
                </crate::button::Button>
            </fieldset>
            <crate::modal::Notice {..}  hidden={move || !uncertain.get()}>
                <p>
                    {move || {
                        i18n.text("message.forge_write_uncertain_read_the_remote_state_atelier_will_not_res")
                    }}
                </p>
                <p>
                    {move || {
                        i18n.text(
                            if intent.get().is_some_and(|intent| intent.draft) {
                                "message.requested_state_draft"
                            } else {
                                "message.requested_state_ready_for_review"
                            },
                        )
                    }}
                </p>
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get() || loading.get())}
                    {..}
                    id="reconcile-forge-draft"
                    type="button"
                    on:click={move |_| change_draft.with_value(|change| change(true))}
                >
                    {move || i18n.text("message.read_remote_draft_state")}
                </crate::button::Button>
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get() || loading.get())}
                    {..}
                    type="button"
                    on:click={move |_| load.with_value(|load| load())}
                >
                    {move || i18n.text("message.reload_forge_connections")}
                </crate::button::Button>
            </crate::modal::Notice>
            <p class="error" id="forge-task-error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="forge-task-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
            {move || {
                intent
                    .get()
                    .filter(|intent| intent.state == ForgeWriteState::Uncertain)
                    .and_then(|intent| intent.content)
                    .map(|content| {
                        view! {
                            <details open>
                                <summary>
                                    {move || i18n.text("message.content_requested_by_the_uncertain_write")}
                                </summary>
                                <p>{content.title}</p>
                                <pre>{content.body}</pre>
                            </details>
                        }
                    })
            }}
            <crate::forge_checks::ForgeChecksView
                card
                configuration={Signal::derive(move || configuration.get())}
                token
            />
            <crate::forge_content::ForgeContentEditor
                card
                configuration={Signal::derive(move || configuration.get())}
                board
                token
                pending
                dirty={content_dirty}
                blocked={Memo::new(move |_| {
                    blocked.get() || draft.get() != original.get() || publication_dirty.get()
                })}
            />
            <crate::button::Button
                disabled={Signal::derive(move || pending.get() || loading.get())}
                {..}
                type="button"
                hidden={move || !unresolved.get()}
                on:click={move |_| load.with_value(|load| load())}
            >
                {move || i18n.text("message.reload_forge_connections")}
            </crate::button::Button>
            <crate::git_push::GitPush
                card
                configuration={Signal::derive(move || configuration.get())}
                board
                token
                pending
                blocked={Memo::new(move |_| {
                    blocked.get() || draft.get() != original.get() || publication_dirty.get()
                        || content_dirty.get()
                })}
                archived
            />
            <crate::forge_publication::ForgePublication
                card
                configuration={Signal::derive(move || configuration.get())}
                board
                token
                pending
                blocked={Memo::new(move |_| {
                    blocked.get() || draft.get() != original.get() || content_dirty.get()
                })}
                dirty={publication_dirty}
                archived
                on_linked={Callback::new(move |pr: PullRequest| {
                    if draft.get_untracked() == original.get_untracked() {
                        let next = (pr.kind, pr.repository, pr.number.to_string());
                        original.set(next.clone());
                        draft.set(next);
                        approved.set(None);
                        detach_ack.set(String::new());
                    }
                })}
            />
        </section>
    }
}
