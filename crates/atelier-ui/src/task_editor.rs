use crate::{STATUSES, field, replace_card, write};
use atelier_core::contracts::Card;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, PartialEq)]
struct Draft {
    title: String,
    description: String,
    instructions: String,
    status: String,
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Archive,
    Duplicate,
    Delete,
}

#[component]
pub fn TaskEditor(
    card: Card,
    cards: RwSignal<Vec<Card>>,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let archived = card.fields.truthy("archived").unwrap_or(false);
    let ticket = card.fields.truthy("ticket").unwrap_or(false);
    let original = StoredValue::new(Draft {
        title: String::from_utf16_lossy(&card.title.0),
        description: field(&card.fields, "description"),
        instructions: field(&card.fields, "instructions"),
        status: field(&card.fields, "status"),
    });
    let id = StoredValue::new(card.id);
    let draft = RwSignal::new(original.get_value());
    let dirty = Memo::new(move |_| draft.get() != original.get_value());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let confirmation = RwSignal::new("");
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            confirmation.set("discard");
        } else {
            close();
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let perform = move |action: Action| {
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
        let mut path = format!("/api/cards/{}", id.get_value());
        let value = draft.get_untracked();
        let (method, body) = match action {
            Action::Save => (
                Method::PATCH,
                serde_json::json!({"title":value.title, "description":value.description, "instructions":value.instructions, "status":value.status}),
            ),
            Action::Archive => {
                path.push_str("/archive");
                (Method::POST, serde_json::json!({"archived":!archived}))
            }
            Action::Duplicate => {
                path.push_str("/duplicate");
                (Method::POST, serde_json::json!({}))
            }
            Action::Delete => (Method::DELETE, serde_json::json!({})),
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&token, method, &path, &body).await?;
                if matches!(action, Action::Delete) {
                    cards.update(|cards| cards.retain(|card| card.id != id.get_value()));
                } else {
                    let card: Card = response
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.")?;
                    replace_card(cards, card);
                }
                Ok::<(), String>(())
            }
            .await;
            pending.set(false);
            match result {
                Ok(()) => close(),
                Err(message) => error.set(message),
            }
        });
    };
    view! {
        <dialog id="task-editor" class="editor" node_ref=dialog aria-labelledby="editor-heading" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{if archived { "ARCHIVE" } else { "TÂCHE" }}</p><h2 id="editor-heading">"Détails de la tâche"</h2></div><button id="close-editor" type="button" disabled=move || pending.get() on:click=move |_| request_close()>"Fermer"</button></header>
            <form on:submit=move |event| { event.prevent_default(); perform(Action::Save); }>
                <fieldset disabled=move || pending.get() || archived>
                    <label for="edit-title">"Titre"</label><input id="edit-title" required maxlength="300" autofocus readonly=ticket prop:value=move || draft.get().title on:input=move |event| draft.update(|draft| draft.title = event_target_value(&event)) />
                    <label for="edit-status">"Statut"</label><select id="edit-status" prop:value=move || draft.get().status on:change=move |event| draft.update(|draft| draft.status = event_target_value(&event))>{STATUSES.into_iter().map(|(status, label)| view! { <option value=status>{label}</option> }).collect_view()}</select>
                    <label for="edit-description">"Description"</label><textarea id="edit-description" rows="5" maxlength="20000" readonly=ticket prop:value=move || draft.get().description on:input=move |event| draft.update(|draft| draft.description = event_target_value(&event))></textarea>
                    <label for="edit-instructions">"Instructions pour l’agent"</label><textarea id="edit-instructions" rows="4" maxlength="20000" prop:value=move || draft.get().instructions on:input=move |event| draft.update(|draft| draft.instructions = event_target_value(&event))></textarea>
                    <p class="field-help">"Changer le statut ne lance pas d’agent."</p>
                </fieldset>
                <p id="editor-error" class="error" role="alert">{move || error.get()}</p>
                <div class="editor-actions"><button id="save-task" class="primary" type="submit" hidden=archived disabled=move || pending.get() || !dirty.get()>{move || if pending.get() { "Enregistrement…" } else { "Enregistrer" }}</button><span role="status">{move || if dirty.get() { "Modifications non enregistrées" } else { "" }}</span></div>
            </form>
            <div class="editor-secondary"><button id="duplicate-task" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| perform(Action::Duplicate)>"Dupliquer"</button><button id="archive-task" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| perform(Action::Archive)>{if archived { "Restaurer" } else { "Archiver" }}</button><button id="delete-task" class="danger" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| confirmation.set("delete")>"Supprimer…"</button></div>
            <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()>
                <p>{move || if confirmation.get() == "delete" { "Supprimer définitivement cette tâche ? Cette action ne peut pas être annulée." } else { "Fermer sans enregistrer les modifications ?" }}</p>
                <button id="confirm-editor" class="danger" type="button" disabled=move || pending.get() on:click=move |_| { if confirmation.get_untracked() == "delete" { perform(Action::Delete); } else { close(); } }>{move || if confirmation.get() == "delete" { "Supprimer définitivement" } else { "Abandonner les modifications" }}</button>
                <button id="cancel-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>"Continuer l’édition"</button>
            </div>
        </dialog>
    }
}
