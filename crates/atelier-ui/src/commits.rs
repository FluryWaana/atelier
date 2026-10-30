use crate::{i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::contracts::{
    Card, ChangeListing, FileChange, ForgeWriteState as Phase, LocalCommitIntent,
    LocalCommitPreview, LocalCommitRequest, LocalCommitResponse,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Commits(
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
    let selected = RwSignal::new(Vec::<String>::new());
    let files = RwSignal::new(Vec::<FileChange>::new());
    let loaded = RwSignal::new(false);
    let page = RwSignal::new(0usize);
    let name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let message = RwSignal::new(String::new());
    let patch = RwSignal::new(String::new());
    let patch_key = RwSignal::new(String::new());
    let approved = RwSignal::new(String::new());
    let awaited = RwSignal::new(None::<LocalCommitPreview>);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let key = Signal::derive(move || {
        serde_json::json!({"card":card.get(),"paths":selected.get(),"name":name.get(),"email":email.get(),"message":message.get()}).to_string()
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
    });
    Effect::new(move |_| {
        dirty.set(
            !selected.get().is_empty()
                || !name.get().is_empty()
                || !email.get().is_empty()
                || !message.get().is_empty(),
        )
    });
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<LocalCommitPreview>("localCommitPreview")
                .ok()
                .flatten()
        })
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<LocalCommitIntent>("localCommit")
                .ok()
                .flatten()
        })
    });
    let workflow_waiting = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            card.fields
                .get::<atelier_core::application::automation::Flow>("automation")
                .ok()
                .flatten()
                .is_some_and(|flow| {
                    flow.state == atelier_core::application::automation::State::Waiting
                })
        })
    });
    Effect::new(move |previous: Option<Option<String>>| {
        let completed = intent
            .get()
            .filter(|intent| intent.state == Phase::Confirmed);
        let operation = completed.as_ref().map(|intent| intent.id.clone());
        if previous.as_ref() != Some(&operation)
            && preview.get_untracked().is_none()
            && let Some(completed) = completed
            && awaited.get_untracked().is_some_and(|expected| {
                expected.snapshot == completed.preview.snapshot
                    && expected.author_name == completed.preview.author_name
                    && expected.author_email == completed.preview.author_email
                    && expected.message == completed.preview.message
            })
            && completed.preview.snapshot.paths == selected.get_untracked()
            && completed.preview.author_name == name.get_untracked()
            && completed.preview.author_email == email.get_untracked()
            && completed.preview.message == message.get_untracked()
        {
            selected.set(Vec::new());
            name.set(String::new());
            email.set(String::new());
            message.set(String::new());
            patch.set(String::new());
            patch_key.set(String::new());
            loaded.set(false);
            awaited.set(None);
        }
        operation
    });
    let uncertain = Memo::new(move |_| {
        intent
            .get()
            .is_some_and(|intent| intent.state == Phase::Uncertain)
    });
    let unresolved = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            atelier_core::application::forge::unresolved(&card.fields).unwrap_or(true)
        })
    });
    let ready = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            preview.snapshot.paths == selected.get()
                && preview.author_name == name.get()
                && preview.author_email == email.get()
                && preview.message == message.get()
        }) && patch_key.get() == key.get()
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked()
            || (action != "reconcile"
                && (blocked.get_untracked() || archived || unresolved.get_untracked()))
        {
            return;
        }
        if matches!(action, "create" | "workflow")
            && (!ready.get_untracked() || approved.get_untracked() != key.get_untracked())
        {
            return;
        }
        if action == "workflow" && !workflow_waiting.get_untracked() {
            return;
        }
        if action == "reconcile" && !uncertain.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let request = LocalCommitRequest {
            expected: expected.clone(),
            paths: selected.get_untracked(),
            author_name: name.get_untracked(),
            author_email: email.get_untracked(),
            message: message.get_untracked(),
            confirmed: action != "preview",
        };
        let path = if action == "workflow" {
            format!("/api/cards/{}/automation", expected.id)
        } else {
            format!("/api/cards/{}/commit/{action}", expected.id)
        };
        let body = if action == "workflow" {
            serde_json::json!({"action":"commit","expected":expected,"confirmed":true})
        } else {
            serde_json::to_value(request).expect("commit request serialization")
        };
        if action == "workflow" {
            awaited.set(preview.get_untracked());
        }
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                if action == "changes" {
                    let listing: ChangeListing = read(&credential, &path)
                        .await?
                        .json()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())?;
                    Ok::<_, String>((None, None, Some(listing.files)))
                } else {
                    let response = write(&credential, Method::POST, &path, &body).await?;
                    if action == "preview" {
                        let response: LocalCommitResponse = response
                            .json()
                            .await
                            .map_err(|_| "message.invalid_server_response".to_owned())?;
                        Ok((Some(response.card), Some(response.patch), None))
                    } else {
                        let response: Card = response
                            .json()
                            .await
                            .map_err(|_| "message.invalid_server_response".to_owned())?;
                        Ok((Some(response), None, None))
                    }
                }
            }
            .await;
            if alive.get() {
                match result {
                    Ok((updated, diff, listing)) => {
                        if let Some(listing) = listing {
                            files.set(listing);
                            loaded.set(true);
                            page.set(0);
                        }
                        if let Some(updated) = updated {
                            let state = updated
                                .fields
                                .get::<LocalCommitIntent>("localCommit")
                                .ok()
                                .flatten();
                            replace_card(board.cards, updated);
                            if let Some(diff) = diff {
                                // Inputs are disabled while pending; the returned card adds only this preview.
                                patch.set(diff);
                                patch_key.set(key.get_untracked());
                                notice.set("message.preview_ready_no_commit_created".into());
                            } else if action == "workflow" {
                                notice.set("message.commit_reserved_in_the_workflow_verification_will_follow_before".into());
                            } else if state.is_some_and(|intent| intent.state == Phase::Confirmed) {
                                if action == "create" {
                                    selected.set(Vec::new());
                                    message.set(String::new());
                                    name.set(String::new());
                                    email.set(String::new());
                                    patch.set(String::new());
                                    patch_key.set(String::new());
                                    loaded.set(false);
                                }
                                notice.set("message.commit_observed_on_the_local_branch_human_files_and_index_are_pr".into());
                            } else {
                                notice.set("message.the_branch_was_read_inspect_the_observed_commit_an_uncertain_res".into());
                            }
                        }
                    }
                    Err(message) => {
                        if action == "workflow" {
                            awaited.set(None);
                        }
                        error.set(message);
                    }
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
            id="local-commit"
            class="task-files"
            aria-labelledby="local-commit-heading"
            hidden={move || card.get().is_none_or(|card| card.fields.raw("worktree").is_none())}
            aria-busy={move || pending.get().to_string()}
        >
            <h4 id="local-commit-heading">{move || i18n.text("message.create_a_local_commit")}</h4>
            <p class="field-help">
                {move || {
                    i18n.text("message.choose_working_files_to_include_the_existing_index_stays_unchang")
                }}
            </p>
            <fieldset disabled={move || pending.get() || blocked.get() || archived || unresolved.get()}>
                <button
                    id="load-commit-files"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute("changes"))}
                >
                    {move || i18n.text("message.refresh_files_to_commit")}
                </button>
                <p hidden={move || {
                    !loaded.get() || !files.get().is_empty()
                }}>{move || i18n.text("message.no_files_changed_since_the_last_commit")}</p>
                <div class="file-list" role="group" aria-label={move || i18n.text("message.commit_files")}>
                    {move || {
                        files
                            .get()
                            .into_iter()
                            .skip(page.get() * 100)
                            .take(100)
                            .map(|file| {
                                let path = StoredValue::new(file.path);
                                view! {
                                    <label>
                                        <input
                                            type="checkbox"
                                            data-commit-path={path.get_value()}
                                            prop:checked={move || {
                                                selected.with(|selected| selected.contains(&path.get_value()))
                                            }}
                                            disabled={move || {
                                                selected
                                                    .with(|selected| {
                                                        selected.len() >= 100
                                                            && !selected.contains(&path.get_value())
                                                    })
                                            }}
                                            on:change={move |event| {
                                                selected
                                                    .update(|selected| {
                                                        let value = path.get_value();
                                                        if event_target_checked(&event) {
                                                            if !selected.contains(&value) {
                                                                selected.push(value);
                                                                selected.sort();
                                                            }
                                                        } else {
                                                            selected.retain(|path| path != &value);
                                                        }
                                                    })
                                            }}
                                        />
                                        <code>{path.get_value()}</code>
                                    </label>
                                }
                            })
                            .collect_view()
                    }}
                </div>
                <div class="editor-actions">
                    <button
                        type="button"
                        disabled={move || page.get() == 0}
                        on:click={move |_| page.update(|page| *page = page.saturating_sub(1))}
                    >
                        {move || i18n.text("message.previous")}
                    </button>
                    <button
                        type="button"
                        disabled={move || (page.get() + 1) * 100 >= files.with(Vec::len)}
                        on:click={move |_| page.update(|page| *page += 1)}
                    >
                        {move || i18n.text("message.next")}
                    </button>
                </div>
                <p>{move || i18n.text("message.commit_selection")}" "{move || selected.get().join(", ")}</p>
                <label for="commit-author-name">{move || i18n.text("message.commit_author")}</label>
                <input
                    id="commit-author-name"
                    maxlength="255"
                    prop:value={move || name.get()}
                    on:input={move |event| name.set(event_target_value(&event))}
                />
                <label for="commit-author-email">{move || i18n.text("message.author_email")}</label>
                <input
                    id="commit-author-email"
                    type="email"
                    maxlength="254"
                    prop:value={move || email.get()}
                    on:input={move |event| email.set(event_target_value(&event))}
                />
                <label for="commit-message">{move || i18n.text("message.commit_message")}</label>
                <textarea
                    id="commit-message"
                    maxlength="2000"
                    prop:value={move || message.get()}
                    on:input={move |event| message.set(event_target_value(&event))}
                />
                <button
                    id="preview-local-commit"
                    type="button"
                    disabled={move || {
                        selected.with(Vec::is_empty) || name.get().trim().is_empty()
                            || email.get().trim().is_empty() || message.get().trim().is_empty()
                    }}
                    on:click={move |_| execute.with_value(|execute| execute("preview"))}
                >
                    {move || i18n.text("message.preview_the_commit")}
                </button>
                <button
                    type="button"
                    on:click={move |_| {
                        selected.set(Vec::new());
                        name.set(String::new());
                        email.set(String::new());
                        message.set(String::new());
                        patch.set(String::new());
                        patch_key.set(String::new());
                    }}
                >
                    {move || i18n.text("message.clear_the_commit_draft")}
                </button>
                <Show when={move || ready.get()}>
                    <pre
                        class="file-preview"
                        tabindex="0"
                        aria-label={move || i18n.text("message.commit_diff")}
                    >
                        {move || patch.get()}
                    </pre>
                    <label>
                        <input
                            id="confirm-local-commit"
                            type="checkbox"
                            prop:checked={move || approved.get() == key.get()}
                            on:change={move |event| {
                                approved
                                    .set(
                                        if event_target_checked(&event) {
                                            key.get_untracked()
                                        } else {
                                            String::new()
                                        },
                                    )
                            }}
                        />
                        {move || i18n.text("message.confirm_this_author_message_and_changes")}
                    </label>
                    <button
                        id="create-local-commit"
                        type="button"
                        disabled={move || approved.get() != key.get()}
                        on:click={move |_| execute.with_value(|execute| execute("create"))}
                    >
                        {move || i18n.text("message.create_the_confirmed_commit")}
                    </button>
                    <button
                        id="workflow-local-commit"
                        type="button"
                        hidden={move || !workflow_waiting.get()}
                        disabled={move || approved.get() != key.get()}
                        on:click={move |_| execute.with_value(|execute| execute("workflow"))}
                    >
                        {move || i18n.text("message.create_this_commit_in_the_workflow_then_verify")}
                    </button>
                </Show>
            </fieldset>
            <div class="confirmation" hidden={move || !uncertain.get()}>
                <p>
                    {move || i18n.text("message.uncertain_commit_read_the_local_branch_before_trying_again")}
                </p>
                <button
                    id="reconcile-local-commit"
                    type="button"
                    disabled={move || pending.get()}
                    on:click={move |_| execute.with_value(|execute| execute("reconcile"))}
                >
                    {move || i18n.text("message.read_the_local_branch")}
                </button>
            </div>
            {move || {
                intent
                    .get()
                    .and_then(|intent| intent.observed)
                    .map(|observed| {
                        view! {
                            <p>
                                {move || i18n.text("message.last_observed_local_commit")}" "
                                <code>{observed}</code>
                            </p>
                        }
                    })
            }}
            <p id="local-commit-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="local-commit-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
        </section>
    }
}
