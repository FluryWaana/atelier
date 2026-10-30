use crate::{i18n::I18n, read, write};
use gloo_net::http::Method;
use leptos::prelude::*;
use serde::Deserialize;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, Deserialize)]
struct Registry {
    revision: u64,
    repositories: Vec<String>,
    available: bool,
}
#[component]
pub fn RepositoryAccess(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    path: Signal<String>,
    id: &'static str,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let registry = RwSignal::new(None::<Registry>);
    let remove_ack = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    Effect::new(move |_| dirty.set(!remove_ack.get().is_empty()));
    Effect::new(move |_| {
        let _ = path.get();
        remove_ack.set(String::new());
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
        let alive = reading.clone();
        spawn_local(async move {
            let result = async {
                read(&credential, "/api/repositories")
                    .await?
                    .json::<Registry>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(value) => registry.set(Some(value)),
                    Err(message) => error.set(message),
                }
                loading.set(false);
            }
        });
    });
    Effect::new(move |_| {
        let selected = path.get();
        if !pending.get() && !selected.trim().is_empty() {
            load.with_value(|load| load());
        }
    });
    let revoking = alive.clone();
    let revoke = StoredValue::new_local(move |selected: String| {
        if pending.get_untracked()
            || loading.get_untracked()
            || remove_ack.get_untracked() != selected
        {
            return;
        }
        let Some(current) = registry
            .get_untracked()
            .filter(|registry| registry.available)
        else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::json!({"path":selected,"expectedRevision":current.revision,"confirmed":true});
        pending.set(true);
        remove_ack.set(String::new());
        error.set(String::new());
        let alive = revoking.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::DELETE, "/api/repositories", &body)
                    .await?
                    .json::<Registry>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(value) => registry.set(Some(value)),
                    Err(message) => error.set(message),
                }
                pending.set(false);
            }
        });
    });
    let save = move |_| {
        if pending.get_untracked() || loading.get_untracked() || path.get_untracked().is_empty() {
            return;
        }
        let Some(current) = registry
            .get_untracked()
            .filter(|registry| registry.available)
        else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::json!({"path":path.get_untracked(),"expectedRevision":current.revision,"confirmed":true});
        pending.set(true);
        error.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, "/api/repositories", &body)
                    .await?
                    .json::<Registry>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(value) => {
                        registry.set(Some(value));
                    }
                    Err(message) => error.set(message),
                }
                pending.set(false);
            }
        });
    };
    let authorized = Memo::new(move |_| {
        registry.get().is_some_and(|registry| {
            registry.repositories.iter().any(|registered| {
                atelier_core::domain::projects::same_repository(
                    registered,
                    &path.get(),
                    registered.as_bytes().get(1) == Some(&b':'),
                )
            })
        })
    });
    view! {
        <div
            class="repository-access"
            id={id}
            aria-busy={move || loading.get().to_string()}
            hidden={move || path.get().trim().is_empty()}
        >
            <p class="field-help" hidden={move || !authorized.get()}>
                {move || i18n.text("message.repository_authorized_here")}
            </p>
            <button
                class="authorize-project-repository"
                type="button"
                hidden={move || authorized.get()}
                disabled={move || {
                    pending.get() || loading.get()
                        || registry.get().is_none_or(|registry| !registry.available)
                }}
                on:click={save}
            >
                {move || i18n.text("message.authorize_this_repository")}
            </button>
            <p class="field-help" hidden={move || authorized.get()}>
                {move || {
                    i18n.text("message.authorize_atelier_to_use_this_repository_and_its_future_worktree")
                }}
            </p>
            <p hidden={move || {
                registry.get().is_none_or(|registry| registry.available)
            }}>{move || i18n.text("message.authorize_repositories_from_the_native_launcher")}</p>
            <details hidden={move || !authorized.get()}>
                <summary>{move || i18n.text("message.remove_authorization")}</summary>
                <label>
                    <input
                        class="confirm-revoke-repository"
                        type="checkbox"
                        disabled={move || pending.get() || loading.get()}
                        prop:checked={move || remove_ack.get() == path.get()}
                        on:change={move |event| {
                            remove_ack
                                .set(
                                    if event_target_checked(&event) {
                                        path.get_untracked()
                                    } else {
                                        String::new()
                                    },
                                )
                        }}
                    />
                    {move || {
                        i18n.text("message.remove_only_this_local_authorization_without_deleting_the_reposi")
                    }}
                </label>
                <button
                    class="revoke-repository"
                    type="button"
                    disabled={move || pending.get() || loading.get() || remove_ack.get() != path.get()}
                    on:click={move |_| revoke.with_value(|revoke| revoke(path.get_untracked()))}
                >
                    {move || i18n.text("message.remove_authorization")}
                </button>
            </details>
            <button
                type="button"
                disabled={move || pending.get() || loading.get()}
                on:click={move |_| load.with_value(|load| load())}
            >
                {move || i18n.text("message.reload_authorizations")}
            </button>
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
        </div>
    }
}
