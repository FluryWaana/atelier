use crate::{field, live::BoardState, write};
use atelier_core::document::Document;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Default, PartialEq)]
struct Draft {
    id: String,
    name: String,
    repo: String,
    rules: String,
    branch: String,
}
impl Draft {
    fn from_document(project: &Document) -> Self {
        Self {
            id: field(project, "id"),
            name: field(project, "name"),
            repo: field(project, "repo"),
            rules: field(project, "projectRules"),
            branch: field(project, "baseBranch"),
        }
    }
}

#[component]
pub fn Projects(
    board: BoardState,
    repository: RwSignal<String>,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let projects = board.projects;
    let original = RwSignal::new(Draft::default());
    let draft = RwSignal::new(Draft::default());
    let dirty = Memo::new(move |_| original.get() != draft.get());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let confirmation = RwSignal::new("");
    let archive = RwSignal::new(false);
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
    let select = move |value: Draft| {
        original.set(value.clone());
        draft.set(value);
        confirmation.set("");
        error.set(String::new());
        archive.set(false);
    };
    let save = move |remove: bool| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let value = draft.get_untracked();
        let previous = original.get_untracked();
        let (method, path) = if remove {
            (Method::DELETE, format!("/api/projects/{}", value.id))
        } else if value.id.is_empty() {
            (Method::POST, "/api/projects".to_owned())
        } else {
            (Method::PATCH, format!("/api/projects/{}", value.id))
        };
        let body = if remove {
            serde_json::json!({"archiveTasks":archive.get_untracked()})
        } else {
            serde_json::json!({"name":value.name, "repo":value.repo, "projectRules":value.rules, "baseBranch":value.branch})
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&token, method, &path, &body).await?;
                if remove {
                    projects.update(|projects| {
                        projects.retain(|project| field(project, "id") != value.id)
                    });
                    if repository.get_untracked() == previous.repo {
                        repository.set(String::new());
                    }
                    select(Draft::default());
                } else {
                    let project: Document = response
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.")?;
                    let saved = Draft::from_document(&project);
                    projects.update(|projects| {
                        if let Some(current) = projects
                            .iter_mut()
                            .find(|project| field(project, "id") == saved.id)
                        {
                            *current = project;
                        } else {
                            projects.push(project);
                        }
                    });
                    repository.set(saved.repo.clone());
                    select(saved);
                }
                // A failed refresh must not turn an already committed creation into a retry.
                board.refresh(&token).await.map_err(|message| {
                    format!("Projet enregistré. Actualisation impossible : {message}")
                })?;
                Ok::<(), String>(())
            }
            .await;
            pending.set(false);
            if let Err(message) = result {
                error.set(message);
            }
        });
    };
    view! {
        <dialog id="projects-dialog" class="editor" node_ref=dialog aria-labelledby="projects-heading" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">"ESPACE DE TRAVAIL"</p><h2 id="projects-heading">"Vos projets"</h2></div><button id="close-projects" type="button" disabled=move || pending.get() on:click=move |_| request_close()>"Fermer"</button></header>
            <div class="project-list" aria-label="Projets enregistrés"><For each=move || projects.get() key=|project| field(project, "id") children=move |project| {
                let id = StoredValue::new(field(&project, "id"));
                let current = move || projects.with(|projects| projects.iter().find(|project| field(project, "id") == id.get_value()).cloned());
                view! { <button type="button" class="project-choice" data-project-id=id.get_value() disabled=move || pending.get() || dirty.get() aria-pressed=move || (draft.get().id == id.get_value()).to_string() on:click=move |_| { if let Some(project) = current() { select(Draft::from_document(&project)); } }><strong>{move || current().map(|project| field(&project,"name")).unwrap_or_default()}</strong><small>{move || current().map(|project| field(&project,"repo")).unwrap_or_default()}</small></button> }
            } /><p class="field-help" hidden=move || !projects.get().is_empty()>"Ajoutez votre premier projet pour regrouper ses tâches et ses réglages."</p></div>
            <button id="new-project" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| select(Draft::default())>"Nouveau projet"</button>
            <form class="project-form" on:submit=move |event| { event.prevent_default(); save(false); }>
                <fieldset disabled=move || pending.get()>
                    <label for="project-name">"Nom du projet"</label><input id="project-name" required maxlength="100" autofocus prop:value=move || draft.get().name on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event)) />
                    <label for="project-repo">"Dossier du dépôt"</label><input id="project-repo" required maxlength="2000" spellcheck="false" prop:value=move || draft.get().repo on:input=move |event| draft.update(|draft| draft.repo = event_target_value(&event)) />
                    <p class="field-help">"Chemin complet d’un dossier existant. Changer ce chemin réassocie les tâches ; les fichiers ne sont pas déplacés."</p>
                    <div hidden=move || draft.get().id.is_empty()><label for="project-branch">"Branche de base"</label><input id="project-branch" maxlength="180" placeholder="Valeur par défaut" prop:value=move || draft.get().branch on:input=move |event| draft.update(|draft| draft.branch = event_target_value(&event)) />
                    <label for="project-rules">"Consignes du projet"</label><textarea id="project-rules" rows="4" maxlength="12000" prop:value=move || draft.get().rules on:input=move |event| draft.update(|draft| draft.rules = event_target_value(&event))></textarea></div>
                </fieldset>
                <p id="project-error" class="error" role="alert">{move || error.get()}</p>
                <div class="editor-actions"><button id="save-project" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || if pending.get() { "Enregistrement…" } else if draft.get().id.is_empty() { "Créer le projet" } else { "Enregistrer" }}</button><button id="reset-project" type="button" disabled=move || pending.get() || !dirty.get() on:click=move |_| confirmation.set("reset")>"Annuler les modifications…"</button></div>
            </form>
            <div class="editor-secondary" hidden=move || draft.get().id.is_empty()><button id="remove-project" class="danger" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| confirmation.set("remove")>"Retirer ce projet…"</button></div>
            <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()><p>{move || if confirmation.get() == "remove" { "Retirer ce projet de la liste ? Les fichiers du dépôt et les tâches sont conservés." } else { "Abandonner les modifications non enregistrées ?" }}</p>
                <label class="check-filter" hidden=move || confirmation.get() != "remove"><input id="archive-project-tasks" type="checkbox" prop:checked=move || archive.get() on:change=move |event| archive.set(event_target_checked(&event)) />"Archiver aussi les tâches de ce projet"</label>
                <button id="confirm-project" class="danger" type="button" disabled=move || pending.get() on:click=move |_| match confirmation.get_untracked() { "remove" => save(true), "reset" => select(original.get_untracked()), _ => close() }>"Confirmer"</button><button id="cancel-project-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>"Revenir"</button>
            </div>
        </dialog>
    }
}
