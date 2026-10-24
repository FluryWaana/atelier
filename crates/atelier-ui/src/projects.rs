use crate::i18n::I18n;
use crate::{field, live::BoardState, write};
use atelier_core::application::projects::EDITABLE_FIELDS;
use atelier_core::document::Document;
use atelier_core::domain::agent_options::providers;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Default, PartialEq)]
struct ServiceDraft {
    repo: String,
    directory: String,
    command: String,
    port: String,
}

#[derive(Clone, Default, PartialEq)]
struct Draft {
    id: String,
    name: String,
    repo: String,
    rules: String,
    branch: String,
    agent: String,
    services: [ServiceDraft; 2],
    linked: String,
}
impl Draft {
    fn from_document(project: &Document) -> Self {
        Self {
            id: field(project, "id"),
            name: field(project, "name"),
            repo: field(project, "repo"),
            rules: field(project, "projectRules"),
            branch: field(project, "baseBranch"),
            agent: field(project, "agent"),
            services: ["front", "back"].map(|role| ServiceDraft {
                repo: field(project, &format!("{role}Repo")),
                directory: field(project, &format!("{role}Directory")),
                command: field(project, &format!("{role}Command")),
                port: project
                    .get::<u16>(&format!("{role}Port"))
                    .ok()
                    .flatten()
                    .unwrap_or(0)
                    .to_string(),
            }),
            linked: field(project, "linkedProjectId"),
        }
    }

    fn patch(&self, previous: &Self) -> serde_json::Value {
        let fields = |value: &Self| {
            let mut fields = serde_json::json!({"name":value.name, "repo":value.repo, "baseBranch":value.branch, "projectRules":value.rules, "agent":value.agent, "linkedProjectId":value.linked});
            for (index, role) in ["front", "back"].into_iter().enumerate() {
                let service = &value.services[index];
                for (suffix, text) in [
                    ("Repo", &service.repo),
                    ("Directory", &service.directory),
                    ("Command", &service.command),
                    ("Port", &service.port),
                ] {
                    fields[format!("{role}{suffix}")] =
                        serde_json::json!(if suffix == "Port" && text.is_empty() {
                            "0"
                        } else {
                            text.as_str()
                        });
                }
            }
            fields
        };
        let (next, previous) = (fields(self), fields(previous));
        let mut patch = serde_json::json!({"expected":{}});
        for field in EDITABLE_FIELDS {
            if next[field] != previous[field] {
                patch[field] = next[field].clone();
                patch["expected"][field] = previous[field].clone();
            }
        }
        patch
    }
}

#[component]
pub fn Projects(
    board: BoardState,
    repository: RwSignal<String>,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
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
        } else if !value.id.is_empty() {
            value.patch(&previous)
        } else {
            serde_json::json!({"name":value.name, "repo":value.repo})
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
            if let Err(message) = result {
                let _ = board.refresh(&token).await;
                error.set(message);
            }
            pending.set(false);
        });
    };
    view! {
        <dialog id="projects-dialog" class="editor" node_ref=dialog aria-labelledby="projects-heading" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text("ESPACE DE TRAVAIL")}</p><h2 id="projects-heading">{move || i18n.text("Vos projets")}</h2></div><button id="close-projects" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            <div class="project-list" aria-label=move || i18n.text("Projets enregistrés")><For each=move || projects.get() key=|project| field(project, "id") children=move |project| {
                let id = StoredValue::new(field(&project, "id"));
                let current = move || projects.with(|projects| projects.iter().find(|project| field(project, "id") == id.get_value()).cloned());
                view! { <button type="button" class="project-choice" data-project-id=id.get_value() disabled=move || pending.get() || dirty.get() aria-pressed=move || (draft.get().id == id.get_value()).to_string() on:click=move |_| { if let Some(project) = current() { select(Draft::from_document(&project)); } }><strong>{move || current().map(|project| field(&project,"name")).unwrap_or_default()}</strong><small>{move || current().map(|project| field(&project,"repo")).unwrap_or_default()}</small></button> }
            } /><p class="field-help" hidden=move || !projects.get().is_empty()>{move || i18n.text("Ajoutez votre premier projet pour regrouper ses tâches et ses réglages.")}</p></div>
            <button id="new-project" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| select(Draft::default())>{move || i18n.text("Nouveau projet")}</button>
            <form class="project-form" on:submit=move |event| { event.prevent_default(); save(false); }>
                <fieldset disabled=move || pending.get()>
                    <label for="project-name">{move || i18n.text("Nom du projet")}</label><input id="project-name" required maxlength="100" autofocus prop:value=move || draft.get().name on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event)) />
                    <label for="project-repo">{move || i18n.text("Dossier du dépôt")}</label><input id="project-repo" required maxlength="2000" spellcheck="false" prop:value=move || draft.get().repo on:input=move |event| draft.update(|draft| draft.repo = event_target_value(&event)) />
                    <p class="field-help">{move || i18n.text("Chemin complet d’un dossier existant. Changer ce chemin réassocie les tâches ; les fichiers ne sont pas déplacés.")}</p>
                    <div hidden=move || draft.get().id.is_empty()><label for="project-branch">{move || i18n.text("Branche de base")}</label><input id="project-branch" maxlength="180" placeholder=move || i18n.text("Valeur par défaut") prop:value=move || draft.get().branch on:input=move |event| draft.update(|draft| draft.branch = event_target_value(&event)) />
                    <label for="project-rules">{move || i18n.text("Consignes du projet")}</label><textarea id="project-rules" rows="4" maxlength="12000" prop:value=move || draft.get().rules on:input=move |event| draft.update(|draft| draft.rules = event_target_value(&event))></textarea>
                    <label for="project-agent">{move || i18n.text("Agent préféré")}</label><select id="project-agent" prop:value=move || draft.get().agent on:change=move |event| draft.update(|draft| draft.agent = event_target_value(&event))><option value="">{move || i18n.text("Hériter du réglage global")}</option><For each=move || providers(&board.settings.get()).unwrap_or_default() key=|provider| field(provider,"id") children=move |provider| view! { <option value=field(&provider,"id")>{field(&provider,"name")}</option> } /></select>
                    <ProjectServices draft=draft pending=pending token=token />
                    <label for="project-linked">{move || i18n.text("Projet lié pour les tests")}</label>
                    <select id="project-linked" prop:value=move || draft.get().linked on:change=move |event| draft.update(|draft| draft.linked = event_target_value(&event))><option value="">{move || i18n.text("Aucun projet lié")}</option><For each={move || projects.get().into_iter().filter(|project| field(project,"id") != draft.get().id).collect::<Vec<_>>()} key=|project| field(project,"id") children=move |project| { let id = field(&project,"id"); view! { <option value=id.clone()>{move || projects.with(|projects| projects.iter().find(|project| field(project,"id") == id).map(|project| field(project,"name")).unwrap_or_default())}</option> } } /></select>
                    <p class="field-help">{move || i18n.text("Les commandes et le projet lié sont enregistrés pour les futurs lancements. Aucun service n’est démarré par cet écran.")}</p></div>
                </fieldset>
                <p id="project-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
                <div class="editor-actions"><button id="save-project" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "Enregistrement…" } else if draft.get().id.is_empty() { "Créer le projet" } else { "Enregistrer" })}</button><button id="reset-project" type="button" disabled=move || pending.get() || !dirty.get() on:click=move |_| confirmation.set("reset")>{move || i18n.text("Annuler les modifications…")}</button></div>
            </form>
            <div class="editor-secondary" hidden=move || draft.get().id.is_empty()><button id="remove-project" class="danger" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| confirmation.set("remove")>{move || i18n.text("Retirer ce projet…")}</button></div>
            <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()><p>{move || i18n.text(if confirmation.get() == "remove" { "Retirer ce projet de la liste ? Les fichiers du dépôt et les tâches sont conservés." } else { "Abandonner les modifications non enregistrées ?" })}</p>
                <label class="check-filter" hidden=move || confirmation.get() != "remove"><input id="archive-project-tasks" type="checkbox" prop:checked=move || archive.get() on:change=move |event| archive.set(event_target_checked(&event)) />{move || i18n.text("Archiver aussi les tâches de ce projet")}</label>
                <button id="confirm-project" class="danger" type="button" disabled=move || pending.get() on:click=move |_| match confirmation.get_untracked() { "remove" => save(true), "reset" => select(original.get_untracked()), _ => close() }>{move || i18n.text("Confirmer")}</button><button id="cancel-project-confirmation" type="button" disabled=move || pending.get() on:click=move |_| confirmation.set("")>{move || i18n.text("Revenir")}</button>
            </div>
        </dialog>
    }
}

#[component]
fn ProjectServices(
    draft: RwSignal<Draft>,
    pending: RwSignal<bool>,
    token: StoredValue<Result<String, String>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        {[(0usize, "front", "Front"), (1usize, "back", "Back")].into_iter().map(move |(index, role, label)| {
            let candidates = RwSignal::new(Vec::<Document>::new());
            let warnings = RwSignal::new(Vec::<String>::new());
            let error = RwSignal::new(String::new());
            let scanned = RwSignal::new(false);
            let identity = Memo::new(move |_| { let value = draft.get(); (value.id, value.repo, value.services[index].repo.clone()) });
            Effect::new(move |_| { let _ = identity.get(); candidates.set(Vec::new()); warnings.set(Vec::new()); error.set(String::new()); scanned.set(false); });
            let detect = move |_| {
                if pending.get_untracked() { return; }
                let value = draft.get_untracked();
                let credential = match token.get_value() { Ok(value) => value, Err(message) => { error.set(message); return; } };
                pending.set(true);
                candidates.set(Vec::new());
                warnings.set(Vec::new());
                error.set(String::new());
                scanned.set(false);
                spawn_local(async move {
                    let result = async {
                        let mut body = serde_json::json!({"role":role,"repo":value.repo});
                        body[format!("{role}Repo")] = serde_json::json!(value.services[index].repo);
                        let response = write(&credential, Method::POST, &format!("/api/projects/{}/detect-launch", value.id), &body).await?;
                        let result: Document = response.json().await.map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        Ok::<_,String>((result.get::<Vec<Document>>("candidates").map_err(|_| "Réponse du serveur invalide.")?.unwrap_or_default(), result.get::<Vec<String>>("warnings").map_err(|_| "Réponse du serveur invalide.")?.unwrap_or_default()))
                    }.await;
                    match result {
                        Ok((found, notices)) => { candidates.set(found); warnings.set(notices); scanned.set(true); }
                        Err(message) => error.set(message),
                    }
                    pending.set(false);
                });
            };
            view! {
                <section class="task-files" aria-labelledby=format!("project-{role}-heading")>
                    <h3 id=format!("project-{role}-heading")>{label}</h3>
                    <label for=format!("project-{role}-repo")>{move || i18n.text("Dépôt du service")}</label><input id=format!("project-{role}-repo") maxlength="2000" spellcheck="false" placeholder=move || i18n.text("Dépôt principal si vide") prop:value=move || draft.get().services[index].repo.clone() on:input=move |event| draft.update(|draft| draft.services[index].repo = event_target_value(&event)) />
                    <label for=format!("project-{role}-directory")>{move || i18n.text("Sous-dossier du service")}</label><input id=format!("project-{role}-directory") maxlength="2000" spellcheck="false" placeholder=move || i18n.text("Racine du worktree si vide") prop:value=move || draft.get().services[index].directory.clone() on:input=move |event| draft.update(|draft| draft.services[index].directory = event_target_value(&event)) />
                    <label for=format!("project-{role}-command")>{move || i18n.text("Commande de lancement")}</label><textarea id=format!("project-{role}-command") rows="2" maxlength="2000" spellcheck="false" prop:value=move || draft.get().services[index].command.clone() on:input=move |event| draft.update(|draft| draft.services[index].command = event_target_value(&event))></textarea>
                    <label for=format!("project-{role}-port")>{move || i18n.text("Port souhaité")}</label><input id=format!("project-{role}-port") type="number" min="0" max="65535" step="1" prop:value=move || draft.get().services[index].port.clone() on:input=move |event| draft.update(|draft| draft.services[index].port = event_target_value(&event)) />
                    <p class="field-help">{move || i18n.text("0 ou vide : port automatique. Utilise {port} dans la commande pour recevoir le port choisi ; une commande vide désactive ce service.")}</p>
                    <button id=format!("detect-{role}") type="button" on:click=detect>{move || i18n.text("Détecter les commandes")}</button>
                    <p class="field-help" hidden=move || !scanned.get() || !candidates.get().is_empty()>{move || i18n.text("Aucune commande détectée. Tu peux saisir ta propre commande.")}</p>
                    <div class="project-list"><For each={move || candidates.get().into_iter().enumerate().collect::<Vec<_>>()} key=|(index,_)| *index children=move |(_, candidate)| {
                        let directory = field(&candidate,"directory");
                        let command = field(&candidate,"command");
                        let runnable = !command.is_empty();
                        let text = format!("{} · {} · {}", field(&candidate,"technology"), if directory.is_empty() { "." } else { &directory }, command);
                        view! { <button class="launch-candidate" data-role=role type="button" disabled=!runnable on:click=move |_| { draft.update(|draft| { draft.services[index].directory = directory.clone(); draft.services[index].command = command.clone(); }); }>{text}</button> }
                    } /></div>
                    <div role="status">{move || warnings.get().into_iter().map(|warning| view! { <p>{i18n.text(&warning)}</p> }).collect_view()}</div>
                    <p class="error" role="alert">{move || i18n.text(&error.get())}</p>
                </section>
            }
        }).collect_view()}
    }
}
