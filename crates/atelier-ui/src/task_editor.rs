use crate::files::Files;
use crate::i18n::I18n;
use crate::worktree::Workspace;
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
    agent: String,
    model: String,
    effort: String,
}

impl Draft {
    fn from_card(card: &Card) -> Self {
        Self {
            title: String::from_utf16_lossy(&card.title.0),
            description: field(&card.fields, "description"),
            instructions: field(&card.fields, "instructions"),
            status: field(&card.fields, "status"),
            agent: field(&card.fields, "agent"),
            model: field(&card.fields, "model"),
            effort: field(&card.fields, "effort"),
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
        }
    }

    fn fields(&self) -> serde_json::Value {
        serde_json::json!({"title":self.title, "description":self.description, "instructions":self.instructions, "status":self.status, "checklist":self.criteria, "blockedBy":self.dependencies,"agent":self.agent,"model":self.model,"effort":self.effort})
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
    on_open: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let cards = board.cards;
    let templates = board.templates;
    let archived = card.fields.truthy("archived").unwrap_or(false);
    let no_repository = field(&card.fields, "repo").is_empty();

    let original = StoredValue::new(Draft::from_card(&card));
    let id = StoredValue::new(card.id);
    let ticket_info = Signal::derive(move || {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .and_then(|card| card.fields.get::<Document>("ticket").ok().flatten())
        })
    });
    let ticket = Memo::new(move |_| ticket_info.with(|ticket| ticket.is_some()));
    let draft = RwSignal::new(original.get_value());
    let dirty = Memo::new(move |_| draft.get() != original.get_value());
    let pending = RwSignal::new(false);
    let workspace_dirty = RwSignal::new(false);
    let time_dirty = RwSignal::new(false);
    let creation_dirty = RwSignal::new(false);
    let forge_dirty = RwSignal::new(false);
    let commit_dirty = RwSignal::new(false);
    let connector_dirty = RwSignal::new(false);
    let reply_drafts = RwSignal::new(std::collections::BTreeSet::<String>::new());
    let show_files = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let confirmation = RwSignal::new("");
    let next_task = RwSignal::new(String::new());
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
    let navigate = Callback::new(move |target: String| {
        if target == id.get_value() {
            return;
        }
        if pending.get_untracked() {
            error.set("Attends la fin de l’opération avant de changer de tâche.".into());
            return;
        }
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || !template_name.get_untracked().is_empty()
        {
            next_task.set(target);
            confirmation.set("switch");
        } else {
            close();
            request_animation_frame(move || on_open.run(target));
        }
    });
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || !template_name.get_untracked().is_empty()
        {
            confirmation.set("discard");
        } else {
            close();
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || pending.get_untracked()
            || !template_name.get_untracked().is_empty()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let perform = move |action: Action| {
        if pending.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
        {
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
    let reload_draft = Callback::new(move |card: Card| {
        if !dirty.get_untracked() {
            let next = Draft::from_card(&card);
            original.set_value(next.clone());
            draft.set(next);
        }
    });
    view! {
        <dialog id="task-editor" class="editor" class:workspace=move || show_files.get() node_ref=dialog aria-labelledby="editor-heading" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text(if archived { "ARCHIVE" } else { "TÂCHE" })}</p><h2 id="editor-heading">{move || i18n.text("Détails de la tâche")}</h2></div><button id="close-editor" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            {move ||ticket_info.get().map(|ticket| {let url=field(&ticket,"url");let key=field(&ticket,"key");let status=field(&ticket,"status");view!{<section class="task-files"><h3>{move || i18n.text("Ticket associé")}</h3>{if url.starts_with("https://") && !url.chars().any(char::is_control) {view!{<a href=url target="_blank" rel="noopener noreferrer">{key}</a>}.into_any()}else{view!{<span>{key}</span>}.into_any()}}<p>{status}</p><p class="field-help">{move || i18n.text("Le titre et la description de cette carte sont conservés localement. Aucune synchronisation automatique n’est active.")}</p></section>}})}
            {move||board.cards.with(|cards|cards.iter().find(|card|card.id==id.get_value()).map(|card|["brainstormQuestions","preparationQuestions"].into_iter().flat_map(|key|card.fields.get::<Vec<String>>(key).ok().flatten().unwrap_or_default()).collect::<Vec<_>>())).filter(|questions|!questions.is_empty()).map(|questions|view!{<section class="task-files"><h3>{move||i18n.text("Questions à clarifier")}</h3><ul>{questions.into_iter().map(|question|view!{<li>{question}</li>}).collect_view()}</ul></section>})}
            <crate::time::TimerBanner board token nested=true on_task=navigate />
            <form on:submit=move |event| { event.prevent_default(); perform(Action::Save); }>
                <fieldset disabled=move || pending.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || archived || show_template.get()>
                    <label for="edit-title">{move || i18n.text("Titre")}</label><input id="edit-title" required maxlength="300" autofocus readonly=move ||ticket.get() prop:value=move || draft.get().title on:input=move |event| draft.update(|draft| draft.title = event_target_value(&event)) />
                    <label for="edit-status">{move || i18n.text("Statut")}</label><select id="edit-status" prop:value=move || draft.get().status on:change=move |event| draft.update(|draft| draft.status = event_target_value(&event))>{STATUSES.into_iter().map(move |(status, label)| view! { <option value=status>{move || i18n.text(label)}</option> }).collect_view()}</select>
                    <label for="edit-description">{move || i18n.text("Description")}</label><textarea id="edit-description" rows="5" maxlength="20000" readonly=move ||ticket.get() prop:value=move || draft.get().description on:input=move |event| draft.update(|draft| draft.description = event_target_value(&event))></textarea>
                    <label for="edit-instructions">{move || i18n.text("Instructions pour l’agent")}</label><textarea id="edit-instructions" rows="4" maxlength="20000" prop:value=move || draft.get().instructions on:input=move |event| draft.update(|draft| draft.instructions = event_target_value(&event))></textarea>
                    <label for="edit-agent">{move||i18n.text("Profil de cette tâche")}</label><select id="edit-agent" prop:value=move||draft.get().agent on:change=move|event|draft.update(|draft|{draft.agent=event_target_value(&event);draft.model.clear();draft.effort.clear();})><option value="" disabled>{move||i18n.text("Choisir un profil")}</option><For each=move||atelier_core::domain::agent_options::providers(&board.settings.get()).unwrap_or_default() key=|profile|field(profile,"id") children=move|profile|view!{<option value=field(&profile,"id")>{field(&profile,"name")}</option>} /></select>
                    <label for="edit-model">{move||i18n.text("Modèle de cette tâche")}</label><input id="edit-model" maxlength="180" spellcheck="false" placeholder=move||i18n.text("Hériter du profil si vide") prop:value=move||draft.get().model on:input=move|event|draft.update(|draft|draft.model=event_target_value(&event)) />
                    <label for="edit-effort">{move||i18n.text("Effort de cette tâche")}</label><input id="edit-effort" maxlength="40" spellcheck="false" placeholder=move||i18n.text("Hériter du profil si vide") prop:value=move||draft.get().effort on:input=move|event|draft.update(|draft|draft.effort=event_target_value(&event)) />
                    <p class="field-help">{move||i18n.text("Changer de profil réinitialise les choix de modèle et d’effort de la tâche. L’enregistrement ne démarre aucun agent.")}</p>
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
                <div class="editor-actions"><button id="save-task" class="primary" type="submit" hidden=archived disabled=move || pending.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || !dirty.get()>{move || i18n.text(if pending.get() { "Enregistrement…" } else { "Enregistrer" })}</button><span role="status">{move || i18n.text(if dirty.get() { "Modifications non enregistrées" } else { "" })}</span></div>
            </form>
            <crate::commits::Commits card_id=id.get_value() board token pending dirty=commit_dirty blocked=Memo::new(move |_|dirty.get()||connector_dirty.get()||workspace_dirty.get()||time_dirty.get()||creation_dirty.get()||forge_dirty.get()||show_template.get()) archived />
            <crate::forge_task::ForgeTask card_id=id.get_value() board token pending dirty=forge_dirty blocked=Memo::new(move |_|dirty.get()||workspace_dirty.get()||time_dirty.get()||creation_dirty.get()||connector_dirty.get()||commit_dirty.get()||show_template.get()) archived />
            <crate::jira_creation::JiraCreation card_id=id.get_value() board token pending dirty=creation_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) archived />
            <crate::time::TimeTask card_id=id.get_value() board token pending dirty=time_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) archived />
            <Show when=move||!no_repository><Workspace card_id=id.get_value() board token pending dirty=workspace_dirty blocked=Memo::new(move |_| dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) on_change=Callback::new(move |()| show_files.set(false)) /></Show>
            <crate::connector_ticket::ConnectorTicket card_id=id.get_value() board token pending dirty=connector_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || commit_dirty.get() || show_template.get() || archived) on_applied=reload_draft />
            <crate::jira_refresh::JiraRefresh card_id=id.get_value() board token pending blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get() || archived) on_applied=reload_draft />
            <crate::jira_task::JiraTask card_id=id.get_value() board token pending blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) />
            <crate::services::Services card_id=id.get_value() board token pending blocked=Memo::new(move |_| dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) />
            <crate::runs::Runs card_id=id.get_value() board token pending reply_drafts on_open=navigate on_applied=reload_draft blocked=Memo::new(move |_| dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get()) />
            <section class="task-files" hidden=no_repository><button id="browse-task-files" type="button" disabled=move || pending.get() aria-expanded=move || show_files.get().to_string() on:click=move |_| show_files.update(|shown| *shown = !*shown)>{move || i18n.text("Fichiers du projet")}</button><Show when=move || show_files.get()><Files card_id=id.get_value() token /></Show></section>
            <div class="editor-secondary"><button id="duplicate-task" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get() on:click=move |_| perform(Action::Duplicate)>{move || i18n.text("Dupliquer")}</button><button id="archive-task" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get() on:click=move |_| perform(Action::Archive)>{move || i18n.text(if archived { "Restaurer" } else { "Archiver" })}</button><button id="delete-task" class="danger" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get() on:click=move |_| confirmation.set("delete")>{move || i18n.text("Supprimer…")}</button><button id="make-template" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || show_template.get() on:click=move |_| { notice.set(String::new()); show_template.set(true); }>{move || i18n.text("Créer un modèle")}</button></div>
            <form class="template-form" hidden=move || !show_template.get() on:submit=move |event| { event.prevent_default(); perform(Action::Template); }><label for="template-name">{move || i18n.text("Nom du modèle")}</label><input id="template-name" required maxlength="100" disabled=move || pending.get() prop:value=move || template_name.get() on:input=move |event| template_name.set(event_target_value(&event)) /><p class="field-help">{move || i18n.text("Le modèle reprend la description, les instructions et les critères à vérifier.")}</p><button id="save-template" class="primary" type="submit" disabled=move || pending.get()>{move || i18n.text("Enregistrer le modèle")}</button><button type="button" disabled=move || pending.get() on:click=move |_| { template_name.set(String::new()); show_template.set(false); }>{move || i18n.text("Annuler le modèle")}</button></form>
            <p id="editor-notice" role="status">{move || i18n.text(&notice.get())}</p>
            <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()>
                <p>{move || i18n.text(if confirmation.get() == "delete" { "Supprimer définitivement cette tâche ? Cette action ne peut pas être annulée." } else { "Fermer sans enregistrer les modifications ?" })}</p>
                <button id="confirm-editor" class="danger" type="button" disabled=move || pending.get() on:click=move |_| { if confirmation.get_untracked() == "delete" { perform(Action::Delete); } else if confirmation.get_untracked()=="switch" {let target=next_task.get_untracked();close();request_animation_frame(move ||on_open.run(target));} else { close(); } }>{move || i18n.text(if confirmation.get() == "delete" { "Supprimer définitivement" } else { "Abandonner les modifications" })}</button>
                <button id="cancel-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>{move || i18n.text("Continuer l’édition")}</button>
            </div>
        </dialog>
    }
}
