use crate::i18n::I18n;
use crate::{STATUSES, field, live::BoardState, replace_card, write};
use atelier_core::contracts::Card;
use atelier_core::document::Document;
use atelier_core::domain::checklist::Criterion;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, PartialEq)]
struct Draft {
    title: String,
    description: String,
    instructions: String,
    status: String,
    criteria: Vec<Criterion>,
    dependencies: Vec<String>,
}

impl Draft {
    fn fields(&self) -> serde_json::Value {
        serde_json::json!({"title":self.title, "description":self.description, "instructions":self.instructions, "status":self.status, "checklist":self.criteria, "blockedBy":self.dependencies})
    }
    fn patch(&self, original: &Self) -> serde_json::Value {
        let current = self.fields();
        let previous = original.fields();
        let mut patch = serde_json::Map::new();
        let mut expected = serde_json::Map::new();
        for (key, value) in current.as_object().expect("draft fields are an object") {
            if value != &previous[key] {
                patch.insert(key.clone(), value.clone());
                expected.insert(key.clone(), previous[key].clone());
            }
        }
        patch.insert("expected".into(), expected.into());
        patch.into()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Archive,
    Duplicate,
    Delete,
    Template,
}

#[component]
pub fn TaskEditor(
    card: Card,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let cards = board.cards;
    let templates = board.templates;
    let archived = card.fields.truthy("archived").unwrap_or(false);
    let ticket = card.fields.truthy("ticket").unwrap_or(false);
    let original = StoredValue::new(Draft {
        title: String::from_utf16_lossy(&card.title.0),
        description: field(&card.fields, "description"),
        instructions: field(&card.fields, "instructions"),
        status: field(&card.fields, "status"),
        criteria: card
            .fields
            .get("checklist")
            .ok()
            .flatten()
            .unwrap_or_default(),
        dependencies: card
            .fields
            .get("blockedBy")
            .ok()
            .flatten()
            .unwrap_or_default(),
    });
    let id = StoredValue::new(card.id);
    let draft = RwSignal::new(original.get_value());
    let dirty = Memo::new(move |_| draft.get() != original.get_value());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let confirmation = RwSignal::new("");
    let show_template = RwSignal::new(false);
    let template_name = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        let target = format!("card-open-{}", id.get_value());
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
        request_animation_frame(move || {
            if let Some(document) = web_sys::window().and_then(|window| window.document())
                && let Some(element) = document
                    .get_element_by_id(&target)
                    .or_else(|| document.get_element_by_id("task-title"))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        });
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() || !template_name.get_untracked().is_empty() {
            confirmation.set("discard");
        } else {
            close();
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked()
            || pending.get_untracked()
            || !template_name.get_untracked().is_empty()
        {
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
            Action::Save => (Method::PATCH, value.patch(&original.get_value())),
            Action::Archive => {
                path.push_str("/archive");
                (Method::POST, serde_json::json!({"archived":!archived}))
            }
            Action::Duplicate => {
                path.push_str("/duplicate");
                (Method::POST, serde_json::json!({}))
            }
            Action::Delete => (Method::DELETE, serde_json::json!({})),
            Action::Template => {
                path = "/api/templates".into();
                (
                    Method::POST,
                    serde_json::json!({"cardId":id.get_value(), "name":template_name.get_untracked()}),
                )
            }
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&token, method, &path, &body).await?;
                if matches!(action, Action::Delete) {
                    cards.update(|cards| cards.retain(|card| card.id != id.get_value()));
                } else if matches!(action, Action::Template) {
                    let template: Document = response
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.")?;
                    templates.update(|templates| {
                        if !templates
                            .iter()
                            .any(|item| field(item, "id") == field(&template, "id"))
                        {
                            templates.push(template);
                        }
                    });
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
            match result {
                Ok(()) => {
                    let _ = board.refresh(&token).await;
                    pending.set(false);
                    if matches!(action, Action::Template) {
                        template_name.set(String::new());
                        show_template.set(false);
                        notice.set("Modèle enregistré.".into());
                    } else {
                        close();
                    }
                }
                Err(message) => {
                    let _ = board.refresh(&token).await;
                    error.set(message);
                    pending.set(false);
                }
            }
        });
    };
    view! {
        <dialog id="task-editor" class="editor" node_ref=dialog aria-labelledby="editor-heading" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text(if archived { "ARCHIVE" } else { "TÂCHE" })}</p><h2 id="editor-heading">{move || i18n.text("Détails de la tâche")}</h2></div><button id="close-editor" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            <form on:submit=move |event| { event.prevent_default(); perform(Action::Save); }>
                <fieldset disabled=move || pending.get() || archived || show_template.get()>
                    <label for="edit-title">{move || i18n.text("Titre")}</label><input id="edit-title" required maxlength="300" autofocus readonly=ticket prop:value=move || draft.get().title on:input=move |event| draft.update(|draft| draft.title = event_target_value(&event)) />
                    <label for="edit-status">{move || i18n.text("Statut")}</label><select id="edit-status" prop:value=move || draft.get().status on:change=move |event| draft.update(|draft| draft.status = event_target_value(&event))>{STATUSES.into_iter().map(move |(status, label)| view! { <option value=status>{move || i18n.text(label)}</option> }).collect_view()}</select>
                    <label for="edit-description">{move || i18n.text("Description")}</label><textarea id="edit-description" rows="5" maxlength="20000" readonly=ticket prop:value=move || draft.get().description on:input=move |event| draft.update(|draft| draft.description = event_target_value(&event))></textarea>
                    <label for="edit-instructions">{move || i18n.text("Instructions pour l’agent")}</label><textarea id="edit-instructions" rows="4" maxlength="20000" prop:value=move || draft.get().instructions on:input=move |event| draft.update(|draft| draft.instructions = event_target_value(&event))></textarea>
                    <p class="field-help">{move || i18n.text("Changer le statut ne lance pas d’agent.")}</p>
                    <section class="criteria" aria-labelledby="criteria-heading"><h3 id="criteria-heading">{move || i18n.text("Critères de réussite")}</h3><p class="field-help">{move || i18n.text("Cochez les critères après votre vérification. Maximum 20 critères.")}</p>
                        <For each={move || (0..draft.get().criteria.len()).collect::<Vec<_>>()} key=|index| *index children=move |index| view! {
                            <div class="criterion"><label><span class="sr-only">{move || i18n.text("Critère {0}").replace("{0}", &(index + 1).to_string())}</span><input class="criterion-text" required maxlength="500" prop:value=move || draft.with(|draft| draft.criteria.get(index).map(|criterion| String::from_utf16_lossy(&criterion.text.0)).unwrap_or_default()) on:input=move |event| draft.update(|draft| { if let Some(criterion) = draft.criteria.get_mut(index) { criterion.text = event_target_value(&event).as_str().into(); } }) /></label>
                            <label class="check-filter"><input class="criterion-done" type="checkbox" prop:checked=move || draft.with(|draft| draft.criteria.get(index).is_some_and(|criterion| criterion.done)) on:change=move |event| draft.update(|draft| { if let Some(criterion) = draft.criteria.get_mut(index) { criterion.done = event_target_checked(&event); } }) />{move || i18n.text("Validé")}</label>
                            <button type="button" aria-label=move || i18n.text("Retirer le critère {0}").replace("{0}", &(index + 1).to_string()) on:click=move |_| draft.update(|draft| { draft.criteria.remove(index); })>{move || i18n.text("Retirer")}</button></div>
                        } />
                        <button id="add-criterion" type="button" disabled={move || draft.get().criteria.len() >= 20} on:click=move |_| draft.update(|draft| draft.criteria.push(Criterion { text: "".into(), done: false }))>{move || i18n.text("Ajouter un critère")}</button>
                    </section>
                    <section class="dependencies" aria-labelledby="dependencies-heading"><h3 id="dependencies-heading">{move || i18n.text("Bloquée par")}</h3><p class="field-help">{move || i18n.text("Choisissez les tâches préalables. Une tâche terminée ne bloque plus le travail.")}</p>
                        <For each={move || cards.get().into_iter().filter(|card| card.id != id.get_value()).collect::<Vec<_>>()} key=|card| card.id.clone() children=move |card| {
                            let dependency = StoredValue::new(card.id);
                            view! { <label class="dependency-choice"><input type="checkbox" data-dependency=dependency.get_value() prop:checked=move || draft.with(|draft| draft.dependencies.contains(&dependency.get_value())) disabled=move || draft.with(|draft| draft.dependencies.len() >= 20 && !draft.dependencies.contains(&dependency.get_value())) on:change=move |event| draft.update(|draft| { let id = dependency.get_value(); if event_target_checked(&event) { if !draft.dependencies.contains(&id) { draft.dependencies.push(id); } } else { draft.dependencies.retain(|value| value != &id); } }) /><span>{String::from_utf16_lossy(&card.title.0)}</span></label> }
                        } />
                        <p class="field-help" hidden={move || cards.get().len() > 1}>{move || i18n.text("Aucune autre tâche disponible.")}</p>
                    </section>
                </fieldset>
                <p id="editor-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
                <div class="editor-actions"><button id="save-task" class="primary" type="submit" hidden=archived disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "Enregistrement…" } else { "Enregistrer" })}</button><span role="status">{move || i18n.text(if dirty.get() { "Modifications non enregistrées" } else { "" })}</span></div>
            </form>
            <div class="editor-secondary"><button id="duplicate-task" type="button" disabled=move || pending.get() || dirty.get() || show_template.get() on:click=move |_| perform(Action::Duplicate)>{move || i18n.text("Dupliquer")}</button><button id="archive-task" type="button" disabled=move || pending.get() || dirty.get() || show_template.get() on:click=move |_| perform(Action::Archive)>{move || i18n.text(if archived { "Restaurer" } else { "Archiver" })}</button><button id="delete-task" class="danger" type="button" disabled=move || pending.get() || dirty.get() || show_template.get() on:click=move |_| confirmation.set("delete")>{move || i18n.text("Supprimer…")}</button><button id="make-template" type="button" disabled=move || pending.get() || dirty.get() || show_template.get() on:click=move |_| { notice.set(String::new()); show_template.set(true); }>{move || i18n.text("Créer un modèle")}</button></div>
            <form class="template-form" hidden=move || !show_template.get() on:submit=move |event| { event.prevent_default(); perform(Action::Template); }><label for="template-name">{move || i18n.text("Nom du modèle")}</label><input id="template-name" required maxlength="100" disabled=move || pending.get() prop:value=move || template_name.get() on:input=move |event| template_name.set(event_target_value(&event)) /><p class="field-help">{move || i18n.text("Le modèle reprend la description, les instructions et les critères à vérifier.")}</p><button id="save-template" class="primary" type="submit" disabled=move || pending.get()>{move || i18n.text("Enregistrer le modèle")}</button><button type="button" disabled=move || pending.get() on:click=move |_| { template_name.set(String::new()); show_template.set(false); }>{move || i18n.text("Annuler le modèle")}</button></form>
            <p id="editor-notice" role="status">{move || i18n.text(&notice.get())}</p>
            <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()>
                <p>{move || i18n.text(if confirmation.get() == "delete" { "Supprimer définitivement cette tâche ? Cette action ne peut pas être annulée." } else { "Fermer sans enregistrer les modifications ?" })}</p>
                <button id="confirm-editor" class="danger" type="button" disabled=move || pending.get() on:click=move |_| { if confirmation.get_untracked() == "delete" { perform(Action::Delete); } else { close(); } }>{move || i18n.text(if confirmation.get() == "delete" { "Supprimer définitivement" } else { "Abandonner les modifications" })}</button>
                <button id="cancel-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>{move || i18n.text("Continuer l’édition")}</button>
            </div>
        </dialog>
    }
}
