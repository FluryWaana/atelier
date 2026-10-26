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
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let registry = RwSignal::new(None::<Registry>);
    let path = RwSignal::new(String::new());
    let approved = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    Effect::new(move |_| dirty.set(!path.get().is_empty()));
    let key = Memo::new(move |_| {
        registry.get().map(|registry| {
            serde_json::json!({"revision":registry.revision,"path":path.get()}).to_string()
        })
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
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
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
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
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
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
        <h3 id="repository-access-heading">{move||i18n.text("Dépôts autorisés sur cet ordinateur")}</h3>
        <p class="field-help">{move||i18n.text("Autorise le dossier principal Git avant de le choisir dans un projet. Cette autorisation locale ne modifie aucun fichier du dépôt et ne lance aucune commande. Les sauvegardes de la base ne la transfèrent pas sur un autre ordinateur.")}</p>
        <ul>{move||registry.get().map(|registry|registry.repositories).unwrap_or_default().into_iter().map(|path|view!{<li><code>{path}</code></li>}).collect_view()}</ul>
        <p hidden=move||registry.get().is_none_or(|registry|registry.available)>{move||i18n.text("Autorise les dépôts depuis le lanceur natif.")}</p>
        <fieldset disabled=move||pending.get()||loading.get()||registry.get().is_none_or(|registry|!registry.available)>
            <label for="repository-access-path">{move||i18n.text("Dossier du dépôt à autoriser")}</label><input id="repository-access-path" maxlength="2000" spellcheck="false" prop:value=move||path.get() on:input=move|event|path.set(event_target_value(&event))/>
            <label><input id="confirm-repository-access" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()})/>{move||i18n.text("Autoriser Atelier à utiliser ce dépôt et ses futurs worktrees après mes confirmations.")}</label>
            <button id="save-repository-access" type="button" disabled=move||path.get().is_empty()||key.get().is_none_or(|key|key!=approved.get()) on:click=save>{move||i18n.text("Autoriser ce dépôt")}</button>
        </fieldset>
        <button type="button" disabled=move||pending.get()||loading.get() on:click=move |_|load.with_value(|load|load())>{move||i18n.text("Recharger les autorisations")}</button>
        <button id="cancel-repository-access" type="button" disabled=move||pending.get()||path.get().is_empty() on:click=move |_|{path.set(String::new());approved.set(String::new());}>{move||i18n.text("Annuler la préparation")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
