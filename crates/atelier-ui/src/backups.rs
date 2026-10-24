use crate::{field, live::BoardState, read, write};
use atelier_core::document::Document;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

async fn list(token: &str) -> Result<Vec<Document>, String> {
    let response: Document = read(token, "/api/backups")
        .await?
        .json()
        .await
        .map_err(|_| "Réponse du serveur invalide.")?;
    response
        .get("backups")
        .map_err(|_| "Liste des sauvegardes invalide.")?
        .ok_or_else(|| "Liste des sauvegardes absente.".into())
}

#[component]
pub fn Backups(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let items = RwSignal::new(Vec::<Document>::new());
    let selected = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if pending.get_untracked() {
            return;
        }
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
    };
    let refresh = move || {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            match list(&token).await {
                Ok(backups) => items.set(backups),
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    refresh();
    let execute = move |restore: bool| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let name = selected.get_untracked();
        if restore && name.is_empty() {
            return;
        }
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            let result = async {
                let (path, body) = if restore {
                    ("/api/backups/restore", serde_json::json!({"name":name,"confirmed":true}))
                } else { ("/api/backups", serde_json::json!({})) };
                let response: Document = write(&token, Method::POST, path, &body).await?.json().await.map_err(|_| "Réponse du serveur invalide.")?;
                notice.set(if restore { format!("Restauration terminée. État précédent conservé dans {}. Les connexions devront être reconfigurées.",field(&response,"rescue")) } else { "Sauvegarde créée.".into() });
                selected.set(String::new());
                if restore { let _ = board.refresh(&token).await; }
                items.set(list(&token).await?);
                Ok::<(), String>(())
            }.await;
            if let Err(message) = result {
                error.set(message);
            }
            pending.set(false);
        });
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <dialog id="backups-dialog" class="editor" node_ref=dialog aria-labelledby="backups-title" on:cancel=move |event: web_sys::Event| { event.prevent_default(); close(); }>
            <header class="editor-heading"><div><p class="eyebrow">"VOS DONNÉES"</p><h2 id="backups-title">"Sauvegardes et restauration"</h2></div><button id="close-backups" type="button" disabled=move || pending.get() on:click=move |_| close()>"Fermer"</button></header>
            <p class="field-help">"Les sauvegardes conservent les données de l’application. Les fichiers des dépôts et worktrees restent à leur emplacement et ne sont pas restaurés."</p>
            <div class="editor-actions"><button id="create-backup" class="primary" type="button" autofocus disabled=move || pending.get() on:click=move |_| execute(false)>"Créer une sauvegarde"</button><button id="refresh-backups" type="button" disabled=move || pending.get() on:click=move |_| refresh()>"Actualiser"</button></div>
            <p id="backups-notice" class="field-help" role="status">{move || if pending.get() { "Opération en cours…".into() } else { notice.get() }}</p><p id="backups-error" class="error" role="alert">{move || error.get()}</p>
            <div class="backup-list" aria-label="Sauvegardes locales" aria-busy=move || pending.get().to_string()><For each=move || items.get() key=|backup| field(backup,"name") children=move |backup| {
                let name = StoredValue::new(field(&backup,"name"));
                let date = field(&backup,"date").replace('T'," ").replace('Z'," UTC");
                let size = backup.get::<u64>("bytes").ok().flatten().unwrap_or(0).div_ceil(1024);
                view! { <button type="button" class="project-choice" data-backup=name.get_value() aria-pressed=move || (selected.get() == name.get_value()).to_string() disabled=move || pending.get() on:click=move |_| selected.set(name.get_value())><strong>{format!("{date} · {size} Kio")}</strong><small>{name.get_value()}</small></button> }
            } /><p class="field-help" hidden=move || pending.get() || !items.get().is_empty()>"Aucune sauvegarde disponible."</p></div>
            <div class="confirmation" role="alert" hidden=move || selected.get().is_empty()><p>"Restaurer cette sauvegarde remplacera les tâches, projets, réglages et temps actuels. Une sauvegarde de secours conservera l’état précédent. Les connexions seront réinitialisées et les automatisations mises en pause."</p><p class="backup-name">{move || selected.get()}</p><button id="restore-backup" class="danger" type="button" disabled=move || pending.get() on:click=move |_| execute(true)>"Restaurer cette sauvegarde"</button><button id="cancel-restore" type="button" disabled=move || pending.get() on:click=move |_| selected.set(String::new())>"Annuler"</button></div>
        </dialog>
    }
}
