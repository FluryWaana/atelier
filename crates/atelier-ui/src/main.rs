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

async fn load(token: &str) -> Result<Vec<Card>, String> {
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
        .map(|board| board.cards)
        .map_err(|_| "Réponse du serveur invalide.".into())
}

#[component]
fn App() -> impl IntoView {
    let cards = RwSignal::new(Vec::<Card>::new());
    let title = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let token = session();
    match &token {
        Ok(token) => {
            let token = token.clone();
            spawn_local(async move {
                match load(&token).await {
                    Ok(value) => cards.set(value),
                    Err(message) => error.set(message),
                }
            });
        }
        Err(message) => error.set(message.clone()),
    }
    view! {
        <main id="main-content" class="main-content">
            <header><p class="eyebrow">"ATELIER KANBAN"</p><h1>"Prototype Rust"</h1><p>"Données de test uniquement · création et lecture après redémarrage"</p></header>
            <form on:submit=move |event| {
                event.prevent_default();
                if pending.get_untracked() { return; }
                let Ok(token) = token.clone() else { return; };
                let entered = title.get_untracked();
                pending.set(true); error.set(String::new());
                spawn_local(async move {
                    let operation = async {
                        let response = Request::post("/api/cards").header("Authorization", &format!("Bearer {token}")).header("X-Local-Request", "1")
                            .json(&serde_json::json!({"title":entered})).map_err(|_| "Requête invalide.".to_owned())?.send().await.map_err(|_| "Connexion interrompue.".to_owned())?;
                        if !response.ok() {
                            let body: serde_json::Value = response.json().await.map_err(|_| "Réponse invalide.".to_owned())?;
                            return Err(body.get("error").and_then(|value| value.as_str()).unwrap_or("Création refusée.").to_owned());
                        }
                        let values = load(&token).await?;
                        cards.set(values); title.set(String::new()); Ok(())
                    }.await;
                    if let Err(message) = operation { error.set(message); }
                    pending.set(false);
                });
            }>
                <label for="task-title">"Titre"</label>
                <input id="task-title" name="title" required maxlength="300" prop:value=move || title.get() on:input=move |event| title.set(event_target_value(&event)) />
                <button id="create-task" class="primary" type="submit" disabled=move || pending.get()>"Créer la tâche"</button>
            </form>
            <p role="alert" id="error">{move || error.get()}</p>
            <section aria-label="Tâches" id="prototype-board">
                <For each=move || cards.get() key=|card| card.id.clone() children=|card| view! { <article class="card" data-id=card.id><CardTitle title=card.title /></article> } />
            </section>
        </main>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
