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
#[derive(Deserialize)]
struct FolderSelection {
    path: Option<String>,
}
#[component]
pub fn RepositoryAccess(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let registry = RwSignal::new(None::<Registry>);
    let path = RwSignal::new(String::new());
    let approved = RwSignal::new(String::new());
    let remove_ack = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    Effect::new(move |_| dirty.set(!path.get().is_empty() || !remove_ack.get().is_empty()));
    let key = Memo::new(move |_| {
        registry.get().map(|registry| {
            serde_json::json!({"revision":registry.revision,"path":path.get()}).to_string()
        })
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
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
        error.set(String::new());
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
    Effect::new(move |_| load.with_value(|load| load()));
    let choosing = alive.clone();
    let choose = move |_| {
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
        let original = path.get_untracked();
        pending.set(true);
        error.set(String::new());
        approved.set(String::new());
        let alive = choosing.clone();
        spawn_local(async move {
            let result = async {
                write(
                    &credential,
                    Method::POST,
                    "/api/folders/choose",
                    &serde_json::json!({"confirmed":true}),
                )
                .await?
                .json::<FolderSelection>()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(value) => {
                        if let Some(value) = value.path
                            && path.get_untracked() == original
                        {
                            path.set(value);
                        }
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
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
        if pending.get_untracked()
            || loading.get_untracked()
            || path.get_untracked().is_empty()
            || key.get_untracked().as_deref() != Some(approved.get_untracked().as_str())
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
        let body = serde_json::json!({"path":path.get_untracked(),"expectedRevision":current.revision,"confirmed":true});
        pending.set(true);
        approved.set(String::new());
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
                        path.set(String::new());
                    }
                    Err(message) => error.set(message),
                }
                pending.set(false);
            }
        });
    };
    view! {<section class="task-files" aria-labelledby="repository-access-heading" aria-busy=move||loading.get().to_string()>
        <h3 id="repository-access-heading">{move||i18n.text("message.repositories_authorized_on_this_computer")}</h3>
        <p class="field-help">{move||i18n.text("message.authorize_the_main_git_directory_before_choosing_it_in_a_project")}</p>
        <ul>{move||registry.get().map(|registry|registry.repositories).unwrap_or_default().into_iter().map(|path|{let selected=StoredValue::new(path.clone());view!{<li><code>{path}</code><label><input class="confirm-revoke-repository" type="checkbox" disabled=move||pending.get()||loading.get()||registry.get().is_none_or(|registry|!registry.available) prop:checked=move||remove_ack.get()==selected.get_value() on:change=move|event|remove_ack.set(if event_target_checked(&event){selected.get_value()}else{String::new()})/>{move||i18n.text("message.remove_only_this_local_authorization_without_deleting_the_reposi")}</label><button class="revoke-repository" type="button" disabled=move||pending.get()||loading.get()||remove_ack.get()!=selected.get_value() on:click=move |_|revoke.with_value(|revoke|revoke(selected.get_value()))>{move||i18n.text("message.remove_authorization")}</button></li>}}).collect_view()}</ul>
        <p hidden=move||registry.get().is_none_or(|registry|registry.available)>{move||i18n.text("message.authorize_repositories_from_the_native_launcher")}</p>
        <fieldset disabled=move||pending.get()||loading.get()||registry.get().is_none_or(|registry|!registry.available)>
            <label for="repository-access-path">{move||i18n.text("message.repository_directory_to_authorize")}</label><input id="repository-access-path" maxlength="2000" spellcheck="false" prop:value=move||path.get() on:input=move|event|path.set(event_target_value(&event))/>
            <button id="choose-repository-folder" type="button" on:click=choose>{move||i18n.text("message.choose_a_folder")}</button>
            <label><input id="confirm-repository-access" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()})/>{move||i18n.text("message.authorize_atelier_to_use_this_repository_and_its_future_worktree")}</label>
            <button id="save-repository-access" type="button" disabled=move||path.get().is_empty()||key.get().is_none_or(|key|key!=approved.get()) on:click=save>{move||i18n.text("message.authorize_this_repository")}</button>
        </fieldset>
        <button type="button" disabled=move||pending.get()||loading.get() on:click=move |_|load.with_value(|load|load())>{move||i18n.text("message.reload_authorizations")}</button>
        <button id="cancel-repository-access" type="button" disabled=move||pending.get()||path.get().is_empty() on:click=move |_|{path.set(String::new());approved.set(String::new());}>{move||i18n.text("message.cancel_preparation")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
