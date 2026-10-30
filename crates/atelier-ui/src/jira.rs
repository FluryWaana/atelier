use crate::{i18n::I18n, live::BoardState, read, write};
use atelier_core::contracts::{
    JiraBatchImport, JiraConfiguration, JiraConfigurationUpdate, JiraConnection, JiraImport,
    JiraImported, JiraIssueDetail, JiraMode, JiraSearch,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

fn focus(id: String) {
    request_animation_frame(move || {
        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(&id))
            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = element.focus();
        }
    });
}

#[component]
pub fn JiraSettings(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(JiraConfiguration::default());
    let draft = RwSignal::new(JiraConfiguration::default());
    let key = RwSignal::new(String::new());
    let clear = RwSignal::new(false);
    let token_set = RwSignal::new(false);
    let loaded = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let searching = RwSignal::new(false);
    let replace = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let results = RwSignal::new(None::<JiraSearch>);
    let page = RwSignal::new(0usize);
    let detail = RwSignal::new(None::<JiraIssueDetail>);
    let inspecting = RwSignal::new(false);
    let inspected_id = RwSignal::new(String::new());
    let import_project = RwSignal::new(String::new());
    let attach_to = RwSignal::new(None::<atelier_core::contracts::Card>);
    let selected = RwSignal::new(std::collections::BTreeSet::<String>::new());
    let batch_approved = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let own_dirty =
        Memo::new(move |_| original.get() != draft.get() || !key.get().is_empty() || clear.get());
    let configured =
        Memo::new(move |_| loaded.get() && token_set.get() && !original.get().url.is_empty());
    let connection_dirty = Memo::new(move |_| {
        let saved = original.get();
        let value = draft.get();
        value.mode != saved.mode
            || value.url != saved.url
            || value.email != saved.email
            || !key.get().is_empty()
            || clear.get()
    });
    Effect::new(move |_| {
        dirty.set(own_dirty.get() || !selected.with(std::collections::BTreeSet::is_empty))
    });
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let load = {
        let alive = alive.clone();
        move || {
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            loading.set(true);
            error.set(String::new());
            replace.set(false);
            results.set(None);
            selected.set(std::collections::BTreeSet::new());
            batch_approved.set(false);
            detail.set(None);
            let alive = alive.clone();
            spawn_local(async move {
                let result = async {
                    read(&credential, "/api/jira/config")
                        .await?
                        .json::<JiraConnection>()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())
                }
                .await;
                if alive.get() {
                    loading.set(false);
                    match result {
                        Ok(connection) => {
                            original.set(connection.configuration.clone());
                            draft.set(connection.configuration);
                            token_set.set(connection.token_set);
                            key.set(String::new());
                            clear.set(false);
                            loaded.set(true);
                        }
                        Err(message) => error.set(message),
                    }
                }
            });
        }
    };
    let load = StoredValue::new_local(load);
    Effect::new(move |_| load.with_value(|load| load()));
    let saving = alive.clone();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let value = draft.get_untracked();
        let body = serde_json::to_value(JiraConfigurationUpdate {
            expected_revision: original.get_untracked().revision,
            mode: value.mode,
            url: value.url,
            email: value.email,
            query: value.query,
            token: (!key.get_untracked().is_empty()).then(|| key.get_untracked()),
            clear_token: clear.get_untracked(),
        })
        .expect("Jira form serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        results.set(None);
        selected.set(std::collections::BTreeSet::new());
        batch_approved.set(false);
        detail.set(None);
        let alive = saving.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::PUT, "/api/jira/config", &body)
                    .await?
                    .json::<JiraConnection>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(connection) => {
                        key.set(String::new());
                        clear.set(false);
                        token_set.set(connection.token_set);
                        original.set(connection.configuration.clone());
                        draft.set(connection.configuration);
                        notice.set("message.jira_connection_saved".into());
                        focus(if token_set.get_untracked() && !original.get_untracked().url.is_empty(){"search-jira"}else{"jira-token"}.into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
        });
    };
    let querying = alive.clone();
    let search = move |_| {
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        replace.set(false);
        searching.set(true);
        error.set(String::new());
        notice.set(String::new());
        results.set(None);
        selected.set(std::collections::BTreeSet::new());
        batch_approved.set(false);
        detail.set(None);
        page.set(0);
        let revision = original.get_untracked().revision;
        let alive = querying.clone();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!("/api/jira/search?revision={revision}"),
                )
                .await?
                .json::<JiraSearch>()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                searching.set(false);
                match result {
                    Ok(value) => results.set(Some(value)),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let inspecting_alive = alive.clone();
    let inspect = StoredValue::new_local(move |id: String| {
        if inspecting.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let revision = original.get_untracked().revision;
        replace.set(false);
        inspected_id.set(id.clone());
        inspecting.set(true);
        detail.set(None);
        error.set(String::new());
        let alive = inspecting_alive.clone();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!("/api/jira/issues/{id}?revision={revision}"),
                )
                .await?
                .json::<JiraIssueDetail>()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                inspecting.set(false);
                if original.get_untracked().revision == revision {
                    match result {
                        Ok(value) => {
                            detail.set(Some(value));
                            focus("jira-detail-heading".into());
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    let importing_alive = alive.clone();
    let import = move |_| {
        let Some(value) = detail.get_untracked() else {
            return;
        };
        if pending.get_untracked() || inspecting.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(JiraImport {
            revision: value.connection_revision,
            issue_id: value.issue.id,
            project_id: import_project.get_untracked(),
            attach_to: attach_to
                .get_untracked()
                .as_ref()
                .map(atelier_core::application::jira::card_snapshot),
        })
        .expect("Jira import serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = importing_alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, "/api/jira/import", &body)
                    .await?
                    .json::<JiraImported>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(result) => notice.set(
                        if result.created {
                            "message.issue_imported_into_a_new_card"
                        } else if result.attached {
                            "message.issue_linked_to_the_card_its_local_content_is_preserved"
                        } else {
                            "message.this_issue_is_already_linked_to_a_card_no_changes_made"
                        }
                        .into(),
                    ),
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
        });
    };
    let batch_alive = alive.clone();
    let import_batch = move |_| {
        if pending.get_untracked()
            || inspecting.get_untracked()
            || searching.get_untracked()
            || own_dirty.get_untracked()
            || !batch_approved.get_untracked()
            || selected.with_untracked(std::collections::BTreeSet::is_empty)
            || import_project.get_untracked().is_empty()
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
        let Some(search) = results.get_untracked() else {
            return;
        };
        let body = serde_json::to_value(JiraBatchImport {
            revision: search.connection_revision,
            issue_ids: selected.with_untracked(|ids| ids.iter().cloned().collect()),
            project_id: import_project.get_untracked(),
            confirmed: true,
        })
        .expect("Jira batch serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = batch_alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, "/api/jira/import-batch", &body)
                    .await?
                    .json::<Vec<JiraImported>>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(_) => {
                        notice.set(
                            "message.batch_imported_new_cards_created_already_linked_cards_preserved"
                                .into(),
                        );
                        selected.set(std::collections::BTreeSet::new());
                        batch_approved.set(false);
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
    view! {
        <section
            class="task-files jira-module"
            aria-labelledby="jira-heading"
            aria-busy={move || {
                (loading.get() || pending.get() || searching.get() || inspecting.get()).to_string()
            }}
        >
            <h3 id="jira-heading">"Jira"</h3>
            <div
                id="jira-connection-summary"
                class="jira-connection-summary"
                hidden={move || !configured.get()}
            >
                <strong>{move || i18n.text("message.jira_connection_saved")}</strong>
                <span>{move || original.get().url}</span>
                <span hidden={move || {
                    original.get().mode != JiraMode::Cloud
                }}>{move || original.get().email}</span>
                <p class="field-help" hidden={move || results.get().is_some()}>
                    {move || i18n.text("message.jira_access_not_checked")}
                </p>
            </div>
            <p role="status" hidden={move || !loading.get()}>
                {move || i18n.text("message.loading_jira_connection")}
            </p>
            <form hidden={move || !loaded.get()} on:submit={save}>
                <fieldset disabled={move || {
                    !selected.with(std::collections::BTreeSet::is_empty) || !loaded.get() || loading.get()
                        || pending.get() || searching.get() || inspecting.get()
                }}>
                    <details
                        id="jira-connection-editor"
                        class="jira-connection-editor"
                        prop:open={move || !configured.get() || connection_dirty.get()}
                    >
                        <summary id="edit-jira-connection">
                            {move || {
                                i18n
                                    .text(
                                        if configured.get() {
                                            "message.edit_jira_connection"
                                        } else {
                                            "message.configure_jira_connection"
                                        },
                                    )
                            }}
                        </summary>
                        <div class="jira-connection-fields">
                            <label for="jira-mode">{move || i18n.text("message.jira_version")}</label>
                            <select
                                id="jira-mode"
                                prop:value={move || {
                                    if draft.get().mode == JiraMode::Cloud { "cloud" } else { "dataCenter" }
                                }}
                                on:change={move |event| {
                                    draft
                                        .update(|draft| {
                                            draft.mode = if event_target_value(&event) == "cloud" {
                                                JiraMode::Cloud
                                            } else {
                                                JiraMode::DataCenter
                                            };
                                        })
                                }}
                            >
                                <option value="cloud">"Jira Cloud"</option>
                                <option value="dataCenter">"Jira Data Center"</option>
                            </select>
                            <label for="jira-url">{move || i18n.text("message.jira_address")}</label>
                            <input
                                id="jira-url"
                                type="url"
                                maxlength="2000"
                                placeholder="https://tickets.example.test"
                                prop:value={move || draft.get().url}
                                on:input={move |event| {
                                    draft.update(|draft| draft.url = event_target_value(&event))
                                }}
                            />
                            <label for="jira-email">{move || i18n.text("message.email_cloud")}</label>
                            <input
                                id="jira-email"
                                type="email"
                                maxlength="300"
                                prop:value={move || draft.get().email}
                                on:input={move |event| {
                                    draft.update(|draft| draft.email = event_target_value(&event))
                                }}
                            />
                            <label for="jira-token">
                                {move || i18n.text("message.api_or_personal_token")}
                            </label>
                            <input
                                id="jira-token"
                                type="password"
                                autocomplete="off"
                                maxlength="4000"
                                disabled={move || clear.get()}
                                prop:value={move || key.get()}
                                on:input={move |event| key.set(event_target_value(&event))}
                            />
                            <p class="field-help">
                                {move || {
                                    i18n
                                        .text(
                                            if token_set.get() {
                                                "message.a_token_is_saved_leave_blank_to_keep_it"
                                            } else {
                                                "message.no_jira_token_saved"
                                            },
                                        )
                                }}
                            </p>
                            <button
                                id="jira-clear-token"
                                type="button"
                                class="danger"
                                hidden={move || !token_set.get()}
                                aria-pressed={move || clear.get().to_string()}
                                on:click={move |_| {
                                    clear.update(|value| *value = !*value);
                                    if clear.get_untracked() {
                                        key.set(String::new());
                                    }
                                }}
                            >
                                {move || {
                                    i18n
                                        .text(
                                            if clear.get() {
                                                "message.cancel_jira_token_removal"
                                            } else {
                                                "message.remove_saved_token"
                                            },
                                        )
                                }}
                            </button>
                            <p class="field-help" hidden={move || !clear.get()}>
                                {move || i18n.text("message.jira_token_removal_pending")}
                            </p>
                            <p class="field-help">
                                {move || {
                                    i18n
                                        .text(
                                            "message.changing_the_jira_address_account_or_version_removes_the_previou",
                                        )
                                }}
                            </p>
                        </div>
                    </details>
                    <label for="jira-query">{move || i18n.text("message.issues_to_search_jql")}</label>
                    <textarea
                        id="jira-query"
                        rows="3"
                        spellcheck="false"
                        maxlength="4000"
                        prop:value={move || draft.get().query}
                        on:input={move |event| draft.update(|draft| draft.query = event_target_value(&event))}
                    />
                </fieldset>
                <button
                    id="save-jira"
                    type="submit"
                    hidden={move || !own_dirty.get()}
                    disabled={move || {
                        !loaded.get() || pending.get() || loading.get() || searching.get() || inspecting.get()
                            || !own_dirty.get()
                    }}
                >
                    {move || i18n.text("message.save_jira_settings")}
                </button>
            </form>
            <div class="editor-actions">
                <button
                    id="search-jira"
                    class="primary"
                    type="button"
                    disabled={move || {
                        !selected.with(std::collections::BTreeSet::is_empty) || !loaded.get()
                            || own_dirty.get() || pending.get() || loading.get() || searching.get()
                            || inspecting.get() || !token_set.get() || original.get().url.is_empty()
                    }}
                    on:click={search}
                >
                    {move || i18n.text("message.search_jira_issues")}
                </button>
                <button
                    id="reload-jira"
                    type="button"
                    hidden={move || loaded.get() && !own_dirty.get() && error.get().is_empty()}
                    disabled={move || pending.get() || loading.get() || searching.get() || inspecting.get()}
                    on:click={move |_| {
                        if own_dirty.get_untracked()
                            || !selected.with_untracked(std::collections::BTreeSet::is_empty)
                        {
                            replace.set(true);
                        } else {
                            load.with_value(|load| load());
                        }
                    }}
                >
                    {move || i18n.text("message.reload_jira_connection")}
                </button>
            </div>
            <div class="confirmation" role="alert" hidden={move || !replace.get()}>
                <p>{move || i18n.text("message.discard_unsaved_changes")}</p>
                <button
                    id="confirm-reload-jira"
                    type="button"
                    on:click={move |_| load.with_value(|load| load())}
                >
                    {move || i18n.text("message.discard")}
                </button>
                <button type="button" on:click={move |_| replace.set(false)}>
                    {move || i18n.text("message.keep_editing")}
                </button>
            </div>
            <p id="jira-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="jira-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
            <p role="status" hidden={move || !searching.get()}>
                {move || i18n.text("message.searching_jira")}
            </p>
            <p hidden={move || {
                !results.with(|results| results.as_ref().is_some_and(|results| results.issues.is_empty()))
            }}>{move || i18n.text("message.no_issues_found")}</p>
            <p role="status" hidden={move || !inspecting.get()}>
                {move || i18n.text("message.loading_jira_issue")}
            </p>
            {move || {
                detail
                    .get()
                    .map(|value| {
                        view! {
                            <section aria-label={move || i18n.text("message.jira_issue_details")}>
                                <h4 id="jira-detail-heading" tabindex="-1">
                                    {value.issue.key}
                                    " · "
                                    {value.issue.title}
                                </h4>
                                <pre>{value.description}</pre>
                                <button
                                    type="button"
                                    disabled={move || pending.get()}
                                    on:click={move |_| {
                                        detail.set(None);
                                        focus(format!("jira-inspect-{}", inspected_id.get_untracked()));
                                    }}
                                >
                                    {move || i18n.text("message.close_details")}
                                </button>
                            </section>
                        }
                    })
            }}
            <div hidden={move || detail.with(|detail| detail.is_none())}>
                <label for="jira-import-project">{move || i18n.text("message.import_into_project")}</label>
                <select
                    id="jira-import-project"
                    disabled={move || pending.get()}
                    prop:value={move || import_project.get()}
                    on:change={move |event| {
                        import_project.set(event_target_value(&event));
                        attach_to.set(None);
                        batch_approved.set(false);
                    }}
                >
                    <option value="">{move || i18n.text("message.choose_a_project")}</option>
                    <For
                        each={move || board.projects.get()}
                        key={|project| project.get::<String>("id").ok().flatten().unwrap_or_default()}
                        children={move |project| {
                            view! {
                                <option value={project
                                    .get::<String>("id")
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default()}>
                                    {project.get::<String>("name").ok().flatten().unwrap_or_default()}
                                </option>
                            }
                        }}
                    />
                </select>
                <label for="jira-attach-card">{move || i18n.text("message.card_to_link")}</label>
                <select
                    id="jira-attach-card"
                    disabled={move || pending.get() || import_project.get().is_empty()}
                    prop:value={move || {
                        attach_to.with(|card| card.as_ref().map(|card| card.id.clone()).unwrap_or_default())
                    }}
                    on:change={move |event| {
                        let id = event_target_value(&event);
                        attach_to
                            .set(board.cards.with(|cards| cards.iter().find(|card| card.id == id).cloned()));
                    }}
                >
                    <option value="">{move || i18n.text("message.create_a_new_card")}</option>
                    <For
                        each={move || {
                            let repo = board
                                .projects
                                .with(|projects| {
                                    projects
                                        .iter()
                                        .find(|project| {
                                            project.get::<String>("id").ok().flatten().as_deref()
                                                == Some(import_project.get().as_str())
                                        })
                                        .and_then(|project| project.get::<String>("repo").ok().flatten())
                                        .unwrap_or_default()
                                });
                            board
                                .cards
                                .with(|cards| {
                                    cards
                                        .iter()
                                        .filter(|card| {
                                            !card.fields.flag("archived").unwrap_or(true)
                                                && card.fields.get::<String>("repo").ok().flatten().as_deref()
                                                    == Some(repo.as_str())
                                        })
                                        .cloned()
                                        .collect::<Vec<_>>()
                                })
                        }}
                        key={|card| card.id.clone()}
                        children={move |card| view! { <option value={card.id}>{card.title.clone()}</option> }}
                    />
                </select>
                <button
                    id="import-jira"
                    type="button"
                    disabled={move || {
                        pending.get() || inspecting.get() || own_dirty.get()
                            || import_project.get().is_empty()
                    }}
                    on:click={import}
                >
                    {move || {
                        i18n
                            .text(
                                if attach_to.with(|card| card.is_some()) {
                                    "message.link_this_issue_to_the_card"
                                } else {
                                    "message.import_this_issue_as_a_card"
                                },
                            )
                    }}
                </button>
                <p class="field-help">
                    {move || {
                        i18n.text("message.the_issue_will_be_read_again_before_importing_an_already_linked")
                    }}
                </p>
            </div>
            <fieldset
                hidden={move || {
                    results.with(|results| results.as_ref().is_none_or(|results| results.issues.is_empty()))
                }}
                disabled={move || pending.get() || inspecting.get() || searching.get() || own_dirty.get()}
            >
                <legend>{move || i18n.text("message.import_selected_jira_issues")}</legend>
                <p class="field-help">
                    {move || {
                        i18n.text("message.select_up_to_20_issues_across_pages_all_will_be_read_again_befor")
                    }}
                </p>
                <p role="status">
                    {move || {
                        i18n
                            .text("message.selected_issues_0")
                            .replace("{0}", &selected.with(|ids| ids.len()).to_string())
                    }}
                </p>
                <label for="jira-batch-project">
                    {move || i18n.text("message.batch_destination_project")}
                </label>
                <select
                    id="jira-batch-project"
                    prop:value={move || import_project.get()}
                    on:change={move |event| {
                        import_project.set(event_target_value(&event));
                        attach_to.set(None);
                        batch_approved.set(false);
                    }}
                >
                    <option value="">{move || i18n.text("message.choose_a_project")}</option>
                    <For
                        each={move || board.projects.get()}
                        key={|project| project.get::<String>("id").ok().flatten().unwrap_or_default()}
                        children={move |project| {
                            view! {
                                <option value={project
                                    .get::<String>("id")
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default()}>
                                    {project.get::<String>("name").ok().flatten().unwrap_or_default()}
                                </option>
                            }
                        }}
                    />
                </select>
                <label>
                    <input
                        id="confirm-jira-batch"
                        type="checkbox"
                        prop:checked={move || batch_approved.get()}
                        on:change={move |event| batch_approved.set(event_target_checked(&event))}
                    />
                    {move || {
                        i18n.text("message.import_this_selection_into_the_chosen_project_and_preserve_alrea")
                    }}
                </label>
                <button
                    id="import-jira-batch"
                    type="button"
                    disabled={move || {
                        !batch_approved.get() || selected.with(std::collections::BTreeSet::is_empty)
                            || import_project.get().is_empty()
                    }}
                    on:click={import_batch}
                >
                    {move || i18n.text("message.confirm_batch_import")}
                </button>
                <button
                    id="clear-jira-batch"
                    type="button"
                    on:click={move |_| {
                        selected.set(std::collections::BTreeSet::new());
                        batch_approved.set(false);
                    }}
                >
                    {move || i18n.text("message.clear_selection")}
                </button>
            </fieldset>
            <ul class="service-endpoints">
                <For
                    each={move || {
                        results
                            .with(|results| {
                                results
                                    .as_ref()
                                    .map(|results| {
                                        results
                                            .issues
                                            .iter()
                                            .skip(page.get() * 200)
                                            .take(200)
                                            .cloned()
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default()
                            })
                    }}
                    key={|issue| issue.id.clone()}
                    children={move |issue| {
                        let id = issue.id.clone();
                        let selection = StoredValue::new(issue.id.clone());
                        view! {
                            <li class="jira-issue">
                                <label>
                                    <input
                                        type="checkbox"
                                        aria-label={move || {
                                            i18n
                                                .text("message.select_issue_0")
                                                .replace("{0}", &selection.get_value())
                                        }}
                                        disabled={move || {
                                            pending.get() || inspecting.get() || searching.get()
                                                || own_dirty.get()
                                                || selected
                                                    .with(|ids| {
                                                        ids.len() >= 20 && !ids.contains(&selection.get_value())
                                                    })
                                        }}
                                        prop:checked={move || {
                                            selected.with(|ids| ids.contains(&selection.get_value()))
                                        }}
                                        on:change={move |event| {
                                            selected
                                                .update(|ids| {
                                                    if event_target_checked(&event) {
                                                        if ids.len() < 20 {
                                                            ids.insert(selection.get_value());
                                                        }
                                                    } else {
                                                        ids.remove(&selection.get_value());
                                                    }
                                                });
                                            batch_approved.set(false);
                                        }}
                                    />
                                    {move || i18n.text("message.select")}
                                </label>
                                <a href={issue.url} target="_blank" rel="noopener noreferrer">
                                    {issue.key}
                                    " · "
                                    {issue.title}
                                </a>
                                " · "
                                {issue.status}
                                <button
                                    id={format!("jira-inspect-{}", issue.id)}
                                    type="button"
                                    disabled={move || {
                                        inspecting.get() || searching.get() || loading.get() || pending.get()
                                            || own_dirty.get()
                                    }}
                                    on:click={move |_| inspect.with_value(|inspect| inspect(id.clone()))}
                                >
                                    {move || i18n.text("message.show_details")}
                                </button>
                            </li>
                        }
                    }}
                />
            </ul>
            <div
                class="editor-actions"
                hidden={move || {
                    results.with(|results| results.as_ref().is_none_or(|results| results.issues.len() <= 200))
                }}
            >
                <button
                    type="button"
                    disabled={move || page.get() == 0}
                    on:click={move |_| page.update(|page| *page = page.saturating_sub(1))}
                >
                    {move || i18n.text("message.previous")}
                </button>
                <button
                    type="button"
                    disabled={move || {
                        results
                            .with(|results| {
                                results
                                    .as_ref()
                                    .is_none_or(|results| (page.get() + 1) * 200 >= results.issues.len())
                            })
                    }}
                    on:click={move |_| page.update(|page| *page += 1)}
                >
                    {move || i18n.text("message.next")}
                </button>
            </div>
        </section>
    }
}
