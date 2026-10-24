use atelier_core::contracts::{Board, Card};
use gloo_net::http::Request;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
fn CardTitle(title: atelier_core::document::JsonString) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H2>::new();
    heading.on_load(move |node| {
        // Keep UTF-16 intact at the DOM boundary; Rust String would replace isolated surrogates.
        let text = js_sys::JsString::from_char_code(&title.0);
        if !js_sys::Reflect::set(node.as_ref(), &"textContent".into(), &text).unwrap_or(false) {
            node.set_text_content(Some("Titre illisible."));
        }
    });
    view! { <h2 class="card-title" node_ref=heading></h2> }
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

async fn load(token: &str) -> Result<Board, String> {
    let response = Request::get("/api/state")
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|_| "Connexion interrompue.")?;
    if !response.ok() {
        return Err("Session expirée ou lecture refusée.".into());
    }
    response
        .json::<Board>()
        .await
        .map_err(|_| "Réponse du serveur invalide.".into())
}

#[component]
fn App() -> impl IntoView {
    let cards = RwSignal::new(Vec::<Card>::new());
    let projects = RwSignal::new(Vec::<atelier_core::document::Document>::new());
    let search = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let archives = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let title = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let token = session();
    match &token {
        Ok(token) => {
            let token = token.clone();
            spawn_local(async move {
                match load(&token).await {
                    Ok(value) => {
                        cards.set(value.cards);
                        projects.set(value.projects);
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
            <header class="app-header"><div><p class="eyebrow">"ATELIER KANBAN"</p><h1>"Votre espace de travail"</h1><p class="subtitle">"Des idées aux tâches terminées, une étape à la fois."</p></div><span class="development">"En développement"</span></header>
            <div class="toolbar" aria-label="Filtres du tableau">
                <div><label for="board-search">"Rechercher"</label><input id="board-search" type="search" placeholder="Titre ou description…" prop:value=move || search.get() on:input=move |event| search.set(event_target_value(&event)) /></div>
                <div><label for="board-project">"Projet"</label><select id="board-project" prop:value=move || repository.get() on:change=move |event| repository.set(event_target_value(&event))><option value="">"Tous les projets"</option><For each=move || projects.get() key=|project| field(project, "id") children=|project| view! { <option value=field(&project, "repo")>{field(&project, "name")}</option> } /></select></div>
                <label class="check-filter"><input id="show-archives" type="checkbox" prop:checked=move || archives.get() on:change=move |event| archives.set(event_target_checked(&event)) />"Archives"</label>
                <span class="task-count" role="status">{move || format!("{} tâche(s)", visible().len())}</span>
            </div>
            <form class="quick-create" on:submit=move |event| {
                event.prevent_default();
                if pending.get_untracked() { return; }
                let Ok(token) = token.clone() else { return; };
                let entered = title.get_untracked();
                let repo = repository.get_untracked();
                pending.set(true); error.set(String::new());
                spawn_local(async move {
                    let operation = async {
                        let mut body = serde_json::json!({"title":entered});
                        if !repo.is_empty() { body["repo"] = serde_json::json!(repo); }
                        let response = Request::post("/api/cards").header("Authorization", &format!("Bearer {token}")).header("X-Local-Request", "1")
                            .json(&body).map_err(|_| "Requête invalide.".to_owned())?.send().await.map_err(|_| "Connexion interrompue.".to_owned())?;
                        if !response.ok() {
                            let body: serde_json::Value = response.json().await.map_err(|_| "Réponse invalide.".to_owned())?;
                            return Err(body.get("error").and_then(|value| value.as_str()).unwrap_or("Création refusée.").to_owned());
                        }
                        let values = load(&token).await?;
                        cards.set(values.cards); projects.set(values.projects); title.set(String::new()); archives.set(false); Ok(())
                    }.await;
                    if let Err(message) = operation { error.set(message); }
                    pending.set(false);
                });
            }>
                <div><label for="task-title">"Nouvelle tâche"</label><input id="task-title" name="title" required maxlength="300" placeholder="Que voulez-vous accomplir ?" prop:value=move || title.get() on:input=move |event| title.set(event_target_value(&event)) /></div>
                <button id="create-task" class="primary" type="submit" disabled=move || pending.get() || loading.get()>{move || if pending.get() { "Création…" } else { "Créer la tâche" }}</button>
            </form>
            <p role="alert" id="error">{move || error.get()}</p>
            <p class="loading" role="status" hidden=move || !loading.get()>"Chargement du tableau…"</p>
            <section aria-label="Tableau des tâches" id="prototype-board" class="kanban" aria-busy=move || loading.get().to_string()>
                <For each=|| vec![("todo", "À faire"), ("doing", "En cours"), ("review", "À vérifier"), ("done", "Terminé"), ("standby", "En attente")] key=|(status, _)| *status children=move |(status, label)| {
                    let column = move || visible().into_iter().filter(|card| field(&card.fields, "status") == status).collect::<Vec<_>>();
                    view! { <section class="kanban-column" data-status=status aria-label=label><header class="column-heading"><h2>{label}</h2><span>{move || column().len()}</span></header><div class="column-cards"><For each=column key=|card| card.id.clone() children=|card| {
                        let description = field(&card.fields, "description");
                        view! { <article class="card" data-id=card.id><CardTitle title=card.title /><p class="card-description">{description}</p></article> }
                    } /><p class="empty-column" hidden=move || !column().is_empty() || loading.get()>"Aucune tâche"</p></div></section> }
                } />
            </section>
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
