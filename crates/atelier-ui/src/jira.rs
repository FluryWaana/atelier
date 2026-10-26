use crate::{i18n::I18n, live::BoardState, read, write};
use atelier_core::contracts::{
    JiraBatchImport, JiraConfiguration, JiraConfigurationUpdate, JiraConnection, JiraImport,
    JiraImported, JiraIssueDetail, JiraMode, JiraSearch,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

fn focus(id: String) {
    request_animation_frame(move || {
        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(&id))
            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = element.focus();
        }
    });
}

#[component]
pub fn JiraSettings(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(JiraConfiguration::default());
    let draft = RwSignal::new(JiraConfiguration::default());
    let key = RwSignal::new(String::new());
    let clear = RwSignal::new(false);
    let token_set = RwSignal::new(false);
    let loaded = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let searching = RwSignal::new(false);
    let replace = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let results = RwSignal::new(None::<JiraSearch>);
    let page = RwSignal::new(0usize);
    let detail = RwSignal::new(None::<JiraIssueDetail>);
    let inspecting = RwSignal::new(false);
    let inspected_id = RwSignal::new(String::new());
    let import_project = RwSignal::new(String::new());
    let attach_to = RwSignal::new(None::<atelier_core::contracts::Card>);
    let selected = RwSignal::new(std::collections::BTreeSet::<String>::new());
    let batch_approved = RwSignal::new(false);
    let sync_dirty = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let own_dirty =
        Memo::new(move |_| original.get() != draft.get() || !key.get().is_empty() || clear.get());
    Effect::new(move |_| {
        dirty.set(
            own_dirty.get()
                || sync_dirty.get()
                || !selected.with(std::collections::BTreeSet::is_empty),
        )
    });
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let load = {
        let alive = alive.clone();
        move || {
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            loading.set(true);
            error.set(String::new());
            replace.set(false);
            results.set(None);
            selected.set(std::collections::BTreeSet::new());
            batch_approved.set(false);
            detail.set(None);
            let alive = alive.clone();
            spawn_local(async move {
                let result = async {
                    read(&credential, "/api/jira/config")
                        .await?
                        .json::<JiraConnection>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                if alive.get() {
                    loading.set(false);
                    match result {
                        Ok(connection) => {
                            original.set(connection.configuration.clone());
                            draft.set(connection.configuration);
                            token_set.set(connection.token_set);
                            key.set(String::new());
                            clear.set(false);
                            loaded.set(true);
                        }
                        Err(message) => error.set(message),
                    }
                }
            });
        }
    };
    let load = StoredValue::new_local(load);
    Effect::new(move |_| load.with_value(|load| load()));
    let saving = alive.clone();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let value = draft.get_untracked();
        let body = serde_json::to_value(JiraConfigurationUpdate {
            expected_revision: original.get_untracked().revision,
            mode: value.mode,
            url: value.url,
            email: value.email,
            query: value.query,
            token: (!key.get_untracked().is_empty()).then(|| key.get_untracked()),
            clear_token: clear.get_untracked(),
        })
        .expect("Jira form serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        results.set(None);
        selected.set(std::collections::BTreeSet::new());
        batch_approved.set(false);
        detail.set(None);
        let alive = saving.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::PUT, "/api/jira/config", &body)
                    .await?
                    .json::<JiraConnection>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(connection) => {
                        key.set(String::new());
                        clear.set(false);
                        token_set.set(connection.token_set);
                        original.set(connection.configuration.clone());
                        draft.set(connection.configuration);
                        notice.set("Connexion Jira enregistrée.".into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
        });
    };
    let querying = alive.clone();
    let search = move |_| {
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        replace.set(false);
        searching.set(true);
        error.set(String::new());
        notice.set(String::new());
        results.set(None);
        selected.set(std::collections::BTreeSet::new());
        batch_approved.set(false);
        detail.set(None);
        page.set(0);
        let revision = original.get_untracked().revision;
        let alive = querying.clone();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!("/api/jira/search?revision={revision}"),
                )
                .await?
                .json::<JiraSearch>()
                .await
                .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                searching.set(false);
                match result {
                    Ok(value) => results.set(Some(value)),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let inspecting_alive = alive.clone();
    let inspect = StoredValue::new_local(move |id: String| {
        if inspecting.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let revision = original.get_untracked().revision;
        replace.set(false);
        inspected_id.set(id.clone());
        inspecting.set(true);
        detail.set(None);
        error.set(String::new());
        let alive = inspecting_alive.clone();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!("/api/jira/issues/{id}?revision={revision}"),
                )
                .await?
                .json::<JiraIssueDetail>()
                .await
                .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                inspecting.set(false);
                if original.get_untracked().revision == revision {
                    match result {
                        Ok(value) => {
                            detail.set(Some(value));
                            focus("jira-detail-heading".into());
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    let importing_alive = alive.clone();
    let import = move |_| {
        let Some(value) = detail.get_untracked() else {
            return;
        };
        if pending.get_untracked() || inspecting.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(JiraImport {
            revision: value.connection_revision,
            issue_id: value.issue.id,
            project_id: import_project.get_untracked(),
            attach_to: attach_to
                .get_untracked()
                .as_ref()
                .map(atelier_core::application::jira::card_snapshot),
        })
        .expect("Jira import serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = importing_alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, "/api/jira/import", &body)
                    .await?
                    .json::<JiraImported>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(result) => notice.set(
                        if result.created {
                            "Ticket importé dans une nouvelle carte."
                        } else if result.attached {
                            "Ticket associé à la carte. Son contenu local est conservé."
                        } else {
                            "Ce ticket est déjà lié à une carte. Aucune modification effectuée."
                        }
                        .into(),
                    ),
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
        });
    };
    let batch_alive = alive.clone();
    let import_batch = move |_| {
        if pending.get_untracked()
            || inspecting.get_untracked()
            || searching.get_untracked()
            || own_dirty.get_untracked()
            || !batch_approved.get_untracked()
            || selected.with_untracked(std::collections::BTreeSet::is_empty)
            || import_project.get_untracked().is_empty()
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let Some(search) = results.get_untracked() else {
            return;
        };
        let body = serde_json::to_value(JiraBatchImport {
            revision: search.connection_revision,
            issue_ids: selected.with_untracked(|ids| ids.iter().cloned().collect()),
            project_id: import_project.get_untracked(),
            confirmed: true,
        })
        .expect("Jira batch serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = batch_alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, "/api/jira/import-batch", &body)
                    .await?
                    .json::<Vec<JiraImported>>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(_) => {
                        notice.set(
                            "Lot importé : nouvelles cartes créées, cartes déjà liées conservées."
                                .into(),
                        );
                        selected.set(std::collections::BTreeSet::new());
                        batch_approved.set(false);
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    view! {
        <section class="task-files" aria-labelledby="jira-heading" aria-busy=move || (loading.get() || pending.get() || searching.get() || inspecting.get()).to_string()>
            <h3 id="jira-heading">"Jira"</h3><p class="field-help">{move || i18n.text("Configure la connexion, puis lance une recherche explicite. Les résultats sont en lecture seule ; aucun ticket distant n’est modifié.")}</p>
            <crate::jira_batch::JiraBatch board token pending dirty=sync_dirty blocked=own_dirty/>
            <p role="status" hidden=move || !loading.get()>{move || i18n.text("Chargement de la connexion Jira…")}</p>
            <form on:submit=save><fieldset disabled=move || !selected.with(std::collections::BTreeSet::is_empty) || !loaded.get() || loading.get() || pending.get() || searching.get() || inspecting.get()>
                <label for="jira-mode">{move || i18n.text("Version de Jira")}</label><select id="jira-mode" prop:value=move || if draft.get().mode==JiraMode::Cloud{"cloud"}else{"dataCenter"} on:change=move |event| draft.update(|draft| draft.mode=if event_target_value(&event)=="cloud"{JiraMode::Cloud}else{JiraMode::DataCenter})><option value="cloud">"Jira Cloud"</option><option value="dataCenter">"Jira Data Center"</option></select>
                <label for="jira-url">{move || i18n.text("Adresse de Jira")}</label><input id="jira-url" type="url" maxlength="2000" placeholder="https://tickets.example.test" prop:value=move || draft.get().url on:input=move |event| draft.update(|draft| draft.url=event_target_value(&event)) />
                <label for="jira-email">{move || i18n.text("E-mail (Cloud)")}</label><input id="jira-email" type="email" maxlength="300" prop:value=move || draft.get().email on:input=move |event| draft.update(|draft| draft.email=event_target_value(&event)) />
                <label for="jira-token">{move || i18n.text("Jeton API ou personnel")}</label><input id="jira-token" type="password" autocomplete="off" maxlength="4000" disabled=move || clear.get() prop:value=move || key.get() on:input=move |event| key.set(event_target_value(&event)) /><p class="field-help">{move || i18n.text(if token_set.get(){"Un jeton est enregistré. Laisser vide pour le conserver."}else{"Aucun jeton Jira enregistré."})}</p>
                <label><input id="jira-clear-token" type="checkbox" prop:checked=move || clear.get() on:change=move |event| {clear.set(event_target_checked(&event));if clear.get_untracked(){key.set(String::new());}} />{move || i18n.text("Retirer le jeton enregistré")}</label>
                <p class="field-help">{move || i18n.text("Changer l’adresse, le compte ou la version de Jira retire le jeton précédent si tu n’en fournis pas un nouveau.")}</p>
                <label for="jira-query">{move || i18n.text("Tickets à rechercher (JQL)")}</label><textarea id="jira-query" maxlength="4000" prop:value=move || draft.get().query on:input=move |event| draft.update(|draft| draft.query=event_target_value(&event)) />
            </fieldset><button id="save-jira" type="submit" disabled=move || !loaded.get() || pending.get() || loading.get() || searching.get() || inspecting.get() || !own_dirty.get()>{move || i18n.text("Enregistrer la connexion Jira")}</button></form>
            <div class="editor-actions"><button id="reload-jira" type="button" disabled=move || pending.get() || loading.get() || searching.get() || inspecting.get() on:click=move |_| {if own_dirty.get_untracked() || !selected.with_untracked(std::collections::BTreeSet::is_empty){replace.set(true);}else{load.with_value(|load|load());}}>{move || i18n.text("Recharger la connexion Jira")}</button><button id="search-jira" type="button" disabled=move || !selected.with(std::collections::BTreeSet::is_empty) || !loaded.get() || own_dirty.get() || pending.get() || loading.get() || searching.get() || inspecting.get() || !token_set.get() || original.get().url.is_empty() on:click=search>{move || i18n.text("Rechercher les tickets Jira")}</button></div>
            <div class="confirmation" role="alert" hidden=move || !replace.get()><p>{move || i18n.text("Abandonner les modifications non enregistrées ?")}</p><button id="confirm-reload-jira" type="button" on:click=move |_| load.with_value(|load|load())>{move || i18n.text("Abandonner")}</button><button type="button" on:click=move |_|replace.set(false)>{move || i18n.text("Continuer à modifier")}</button></div>
            <p id="jira-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="jira-notice" role="status">{move || i18n.text(&notice.get())}</p>
            <p role="status" hidden=move || !searching.get()>{move || i18n.text("Recherche Jira en cours…")}</p>
            <p hidden=move || !results.with(|results| results.as_ref().is_some_and(|results| results.issues.is_empty()))>{move || i18n.text("Aucun ticket trouvé.")}</p>
            <p role="status" hidden=move || !inspecting.get()>{move || i18n.text("Chargement du ticket Jira…")}</p>
            {move || detail.get().map(|value| view!{<section aria-label=move || i18n.text("Détail du ticket Jira")><h4 id="jira-detail-heading" tabindex="-1">{value.issue.key}" · "{value.issue.title}</h4><pre>{value.description}</pre><button type="button" disabled=move || pending.get() on:click=move |_|{detail.set(None);focus(format!("jira-inspect-{}",inspected_id.get_untracked()));}>{move || i18n.text("Fermer le détail")}</button></section>})}
            <div hidden=move || detail.with(|detail|detail.is_none())><label for="jira-import-project">{move || i18n.text("Importer dans le projet")}</label><select id="jira-import-project" disabled=move || pending.get() prop:value=move || import_project.get() on:change=move |event|{import_project.set(event_target_value(&event));attach_to.set(None);batch_approved.set(false);}><option value="">{move || i18n.text("Choisir un projet")}</option><For each=move || board.projects.get() key=|project|project.get::<String>("id").ok().flatten().unwrap_or_default() children=move |project|view!{<option value=project.get::<String>("id").ok().flatten().unwrap_or_default()>{project.get::<String>("name").ok().flatten().unwrap_or_default()}</option>} /></select><label for="jira-attach-card">{move || i18n.text("Carte à associer")}</label><select id="jira-attach-card" disabled=move || pending.get() || import_project.get().is_empty() prop:value=move || attach_to.with(|card|card.as_ref().map(|card|card.id.clone()).unwrap_or_default()) on:change=move |event|{let id=event_target_value(&event);attach_to.set(board.cards.with(|cards|cards.iter().find(|card|card.id==id).cloned()));}><option value="">{move || i18n.text("Créer une nouvelle carte")}</option><For each=move || {let repo=board.projects.with(|projects|projects.iter().find(|project|project.get::<String>("id").ok().flatten().as_deref()==Some(import_project.get().as_str())).and_then(|project|project.get::<String>("repo").ok().flatten()).unwrap_or_default());board.cards.with(|cards|cards.iter().filter(|card|!card.fields.truthy("archived").unwrap_or(true) && card.fields.get::<String>("repo").ok().flatten().as_deref()==Some(repo.as_str())).cloned().collect::<Vec<_>>())} key=|card|card.id.clone() children=move |card|view!{<option value=card.id>{String::from_utf16_lossy(&card.title.0)}</option>} /></select><button id="import-jira" type="button" disabled=move || pending.get() || inspecting.get() || own_dirty.get() || import_project.get().is_empty() on:click=import>{move || i18n.text(if attach_to.with(|card|card.is_some()) {"Associer ce ticket à la carte"}else{"Importer ce ticket en carte"})}</button><p class="field-help">{move || i18n.text("Le ticket sera relu avant l’import. Une carte déjà liée sera conservée sans modification.")}</p></div>
            <fieldset hidden=move||results.with(|results|results.as_ref().is_none_or(|results|results.issues.is_empty())) disabled=move||pending.get()||inspecting.get()||searching.get()||own_dirty.get()>
                <legend>{move||i18n.text("Importer une sélection Jira")}</legend>
                <p class="field-help">{move||i18n.text("Sélectionne jusqu’à 20 tickets, y compris sur plusieurs pages. Ils seront tous relus avant une seule transaction ; en cas d’erreur, aucune nouvelle carte ne sera conservée.")}</p>
                <p role="status">{move||i18n.text("Tickets sélectionnés : {0}").replace("{0}",&selected.with(|ids|ids.len()).to_string())}</p>
                <label for="jira-batch-project">{move||i18n.text("Projet de destination du lot")}</label><select id="jira-batch-project" prop:value=move||import_project.get() on:change=move|event|{import_project.set(event_target_value(&event));attach_to.set(None);batch_approved.set(false);} ><option value="">{move||i18n.text("Choisir un projet")}</option><For each=move||board.projects.get() key=|project|project.get::<String>("id").ok().flatten().unwrap_or_default() children=move|project|view!{<option value=project.get::<String>("id").ok().flatten().unwrap_or_default()>{project.get::<String>("name").ok().flatten().unwrap_or_default()}</option>}/></select>
                <label><input id="confirm-jira-batch" type="checkbox" prop:checked=move||batch_approved.get() on:change=move|event|batch_approved.set(event_target_checked(&event)) />{move||i18n.text("Importer cette sélection dans le projet choisi et conserver les cartes déjà liées.")}</label>
                <button id="import-jira-batch" type="button" disabled=move||!batch_approved.get()||selected.with(std::collections::BTreeSet::is_empty)||import_project.get().is_empty() on:click=import_batch>{move||i18n.text("Confirmer l’import du lot")}</button>
                <button id="clear-jira-batch" type="button" on:click=move|_|{selected.set(std::collections::BTreeSet::new());batch_approved.set(false);}>{move||i18n.text("Vider la sélection")}</button>
            </fieldset>
            <ul class="service-endpoints"><For each=move || results.with(|results| results.as_ref().map(|results| results.issues.iter().skip(page.get()*200).take(200).cloned().collect::<Vec<_>>()).unwrap_or_default()) key=|issue| issue.id.clone() children=move |issue| {let id=issue.id.clone(); let selection=StoredValue::new(issue.id.clone()); view!{<li class="jira-issue"><label><input type="checkbox" aria-label=move||i18n.text("Sélectionner le ticket {0}").replace("{0}",&selection.get_value()) disabled=move||pending.get()||inspecting.get()||searching.get()||own_dirty.get()||selected.with(|ids|ids.len()>=20&&!ids.contains(&selection.get_value())) prop:checked=move||selected.with(|ids|ids.contains(&selection.get_value())) on:change=move|event|{selected.update(|ids|{if event_target_checked(&event){if ids.len()<20{ids.insert(selection.get_value());}}else{ids.remove(&selection.get_value());}});batch_approved.set(false);} />{move||i18n.text("Sélectionner")}</label><a href=issue.url target="_blank" rel="noopener noreferrer">{issue.key}" · "{issue.title}</a>" · "{issue.status}<button id=format!("jira-inspect-{}",issue.id) type="button" disabled=move || inspecting.get() || searching.get() || loading.get() || pending.get() || own_dirty.get() on:click=move |_|inspect.with_value(|inspect|inspect(id.clone()))>{move || i18n.text("Afficher le détail")}</button></li>}} /></ul>
            <div class="editor-actions" hidden=move || results.with(|results| results.as_ref().is_none_or(|results| results.issues.len()<=200))><button type="button" disabled=move || page.get()==0 on:click=move |_|page.update(|page|*page=page.saturating_sub(1))>{move || i18n.text("Précédent")}</button><button type="button" disabled=move || results.with(|results| results.as_ref().is_none_or(|results| (page.get()+1)*200>=results.issues.len())) on:click=move |_|page.update(|page|*page+=1)>{move || i18n.text("Suivant")}</button></div>
        </section>
    }
}
