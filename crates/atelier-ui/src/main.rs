use crate::i18n::I18n;
use atelier_core::contracts::{Board, Card};
use gloo_net::http::{Method, Request, RequestBuilder, Response};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

mod agent_diagnostics;
mod agent_profiles;
mod backups;
mod files;
mod i18n;
mod jira;
mod live;
mod projects;
mod runs;
mod services;
mod settings;
mod task_editor;
mod worktree;
use agent_profiles::AgentProfiles;
use backups::Backups;
use live::BoardState;
use projects::Projects;
use settings::Settings;
use task_editor::TaskEditor;

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

fn task_location(id: Option<&str>) -> Result<(), String> {
    let window = web_sys::window().ok_or("Navigateur indisponible.")?;
    let location = window.location();
    let params = web_sys::UrlSearchParams::new_with_str(
        &location.search().map_err(|_| "Adresse invalide.")?,
    )
    .map_err(|_| "Adresse invalide.")?;
    match id {
        Some(id) => params.set("task", id),
        None => params.delete("task"),
    }
    let query = String::from(params.to_string());
    let address = format!(
        "{}{}{}",
        location.pathname().map_err(|_| "Adresse invalide.")?,
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        },
        location.hash().map_err(|_| "Adresse invalide.")?
    );
    // Editing is a modal state: keep one history entry and make its address reloadable.
    window
        .history()
        .and_then(|history| {
            history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&address))
        })
        .map_err(|_| "Adresse de tâche indisponible.".into())
}

fn requested_task() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get("task")
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
    params.delete("session");
    let fragment = String::from(params.to_string());
    let address = format!(
        "{}{}{}",
        location.pathname().map_err(|_| "Adresse invalide.")?,
        location.search().map_err(|_| "Adresse invalide.")?,
        if fragment.is_empty() {
            String::new()
        } else {
            format!("#{fragment}")
        }
    );
    window
        .history()
        .and_then(|history| {
            history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&address))
        })
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
    let show_settings = RwSignal::new(false);
    let show_agents = RwSignal::new(false);
    let show_backups = RwSignal::new(false);
    let cards = board.cards;
    let projects = board.projects;
    let templates = board.templates;
    let template = RwSignal::new(String::new());
    let remove_template = RwSignal::new(false);
    let template_pending = RwSignal::new(false);
    let search = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let archives = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let title = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let selected = RwSignal::new(None::<Card>);
    let show_projects = RwSignal::new(false);
    let token = StoredValue::new(session());
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
    let visible = move || {
        let query = search.get().trim().to_lowercase();
        let repo = repository.get();
        cards
            .get()
            .into_iter()
            .filter(|card| {
                card.fields.truthy("archived").unwrap_or(false) == archives.get()
                    && (repo.is_empty() || field(&card.fields, "repo") == repo)
                    && (query.is_empty()
                        || format!(
                            "{} {}",
                            String::from_utf16_lossy(&card.title.0),
                            field(&card.fields, "description")
                        )
                        .to_lowercase()
                        .contains(&query))
            })
            .collect::<Vec<_>>()
    };
    view! {
        <main id="main-content" class="main-content">
            <header class="app-header"><div><p class="eyebrow">"ATELIER KANBAN"</p><h1>{move || i18n.text("Votre espace de travail")}</h1><p class="subtitle">{move || i18n.text("Des idées aux tâches terminées, une étape à la fois.")}</p></div><span class="development">{move || i18n.text("En développement")}</span></header>
            <div class="toolbar" aria-label=move || i18n.text("Filtres du tableau")>
                <div><label for="board-search">{move || i18n.text("Rechercher")}</label><input id="board-search" type="search" placeholder=move || i18n.text("Titre ou description…") prop:value=move || search.get() on:input=move |event| search.set(event_target_value(&event)) /></div>
                <div><label for="board-project">{move || i18n.text("Projet")}</label><select id="board-project" prop:value=move || repository.get() on:change=move |event| repository.set(event_target_value(&event))><option value="">{move || i18n.text("Tous les projets")}</option><For each=move || projects.get() key=|project| field(project, "id") children=move |project| {
                    let id = StoredValue::new(field(&project, "id"));
                    let current = move || projects.with(|projects| projects.iter().find(|project| field(project, "id") == id.get_value()).cloned());
                    view! { <option value=move || current().map(|project| field(&project, "repo")).unwrap_or_default()>{move || current().map(|project| field(&project, "name")).unwrap_or_default()}</option> }
                } /></select></div>
                <label class="check-filter"><input id="show-archives" type="checkbox" prop:checked=move || archives.get() on:change=move |event| archives.set(event_target_checked(&event)) />{move || i18n.text("Archives")}</label>
                <button id="manage-projects" type="button" disabled=move || loading.get() on:click=move |_| show_projects.set(true)>{move || i18n.text("Gérer les projets")}</button>
                <button id="manage-settings" type="button" disabled=move || loading.get() on:click=move |_| show_settings.set(true)>{move || i18n.text("Paramètres")}</button>
                <button id="manage-agents" type="button" disabled=move || loading.get() on:click=move |_| show_agents.set(true)>{move || i18n.text("Agents")}</button>
                <button id="manage-backups" type="button" disabled=move || loading.get() on:click=move |_| show_backups.set(true)>{move || i18n.text("Sauvegardes")}</button>
                <span class="task-count" role="status">{move || i18n.text("{0} tâche(s)").replace("{0}", &visible().len().to_string())}</span>
            </div>
            <form class="quick-create" on:submit=move |event| {
                event.prevent_default();
                if pending.get_untracked() || template_pending.get_untracked() { return; }
                let Ok(token) = token.get_value() else { return; };
                let entered = title.get_untracked();
                let repo = repository.get_untracked();
                let selected_template = template.get_untracked();
                pending.set(true); error.set(String::new());
                spawn_local(async move {
                    let operation = async {
                        let mut body = serde_json::json!({"title":entered});
                        if !repo.is_empty() { body["repo"] = serde_json::json!(repo); }
                        if !selected_template.is_empty() { body["templateId"] = serde_json::json!(selected_template); }
                        let response = write(&token, Method::POST, "/api/cards", &body).await?;
                        let card: Card = response.json().await.map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        replace_card(cards, card); title.set(String::new()); archives.set(false); let _ = board.refresh(&token).await; Ok::<(), String>(())
                    }.await;
                    if let Err(message) = operation { error.set(message); }
                    pending.set(false);
                });
            }>
                <div><label for="task-title">{move || i18n.text("Nouvelle tâche")}</label><input id="task-title" name="title" required maxlength="300" disabled=move || pending.get() placeholder=move || i18n.text("Que voulez-vous accomplir ?") prop:value=move || title.get() on:input=move |event| title.set(event_target_value(&event)) /></div>
                <div class="template-picker" hidden=move || templates.get().is_empty()><label for="task-template">{move || i18n.text("Modèle")}</label><select id="task-template" disabled=move || pending.get() || template_pending.get() prop:value=move || template.get() on:change=move |event| { template.set(event_target_value(&event)); remove_template.set(false); }><option value="">{move || i18n.text("Sans modèle")}</option><For each=move || templates.get() key=|item| field(item,"id") children=|item| view! { <option value=field(&item,"id")>{field(&item,"name")}</option> } /></select><button id="remove-template" type="button" class="text-action" hidden=move || template.get().is_empty() disabled=move || pending.get() || template_pending.get() on:click=move |_| remove_template.set(true)>{move || i18n.text("Retirer ce modèle…")}</button></div>
                <button id="create-task" class="primary" type="submit" disabled=move || pending.get() || template_pending.get() || loading.get()>{move || i18n.text(if pending.get() { "Création…" } else { "Créer la tâche" })}</button>
            </form>
            <div class="confirmation" hidden=move || !remove_template.get()><p>{move || i18n.text("Retirer ce modèle ? Les tâches existantes sont conservées.")}</p><button id="confirm-remove-template" type="button" disabled=move || template_pending.get() on:click=move |_| {
                if template_pending.get_untracked() { return; }
                let Ok(token) = token.get_value() else { return; };
                let id = template.get_untracked(); template_pending.set(true); error.set(String::new());
                spawn_local(async move {
                    match write(&token, Method::DELETE, &format!("/api/templates/{id}"), &serde_json::json!({})).await {
                        Ok(_) => { templates.update(|items| items.retain(|item| field(item,"id") != id)); template.set(String::new()); remove_template.set(false); let _ = board.refresh(&token).await; },
                        Err(message) => error.set(message),
                    }
                    template_pending.set(false);
                });
            }>{move || i18n.text("Retirer le modèle")}</button><button type="button" disabled=move || template_pending.get() on:click=move |_| remove_template.set(false)>{move || i18n.text("Annuler")}</button></div>
            <p role="alert" id="error">{move || i18n.text(&error.get())}</p>
            <p id="live-status" class="field-help" role="status" hidden=move || token.get_value().is_err() data-connected=move || board.connected.get().to_string()>{move || i18n.text(&(if !board.sync_error.get().is_empty() { board.sync_error.get() } else if board.connected.get() { "Tableau à jour".to_owned() } else { "Actualisation interrompue. Reconnexion automatique…".to_owned() }))}</p>
            <p class="loading" role="status" hidden=move || !loading.get()>{move || i18n.text("Chargement du tableau…")}</p>
            <section aria-label=move || i18n.text("Tableau des tâches") id="prototype-board" class="kanban" aria-busy=move || loading.get().to_string()>
                <For each=|| STATUSES key=|(status, _)| *status children=move |(status, label)| {
                    let column = move || visible().into_iter().filter(|card| field(&card.fields, "status") == status).collect::<Vec<_>>();
                    view! { <section class="kanban-column" data-status=status aria-label=move || i18n.text(label)><header class="column-heading"><h2>{move || i18n.text(label)}</h2><span>{move || column().len()}</span></header><div class="column-cards"><For each=column key=|card| card.id.clone() children=move |card| {
                        let id = StoredValue::new(card.id);
                        let current = move || cards.with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned());
                        view! { <article class="card" data-id=id.get_value()><h3 class="card-title"><button type="button" class="card-open" id=format!("card-open-{}", id.get_value()) on:click=move |_| { error.set(String::new()); if let Err(message) = task_location(Some(&id.get_value())) { error.set(message); } selected.set(current()); }>{move || current().map(|card| String::from_utf16_lossy(&card.title.0)).unwrap_or_default()}</button></h3><p class="card-description">{move || current().map(|card| field(&card.fields, "description")).unwrap_or_default()}</p></article> }
                    } /><p class="empty-column" hidden=move || !column().is_empty() || loading.get()>{move || i18n.text("Aucune tâche")}</p></div></section> }
                } />
            </section>
            {move || selected.get().map(|card| view! { <TaskEditor card board token on_close=Callback::new(move |()| { selected.set(None); if let Err(message) = task_location(None) { error.set(message); } }) /> })}
            <Show when=move || show_projects.get()><Projects board repository token on_close=Callback::new(move |()| show_projects.set(false)) /></Show>
            <Show when=move || show_settings.get()><Settings board token on_close=Callback::new(move |()| show_settings.set(false)) /></Show>
            <Show when=move || show_agents.get()><AgentProfiles board token on_close=Callback::new(move |()| show_agents.set(false)) /></Show>
            <Show when=move || show_backups.get()><Backups board token on_close=Callback::new(move |()| show_backups.set(false)) /></Show>
        </main>
    }
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
