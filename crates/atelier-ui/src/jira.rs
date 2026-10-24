use crate::{i18n::I18n, live::BoardState, read, write};
use atelier_core::contracts::{
    JiraConfiguration, JiraConfigurationUpdate, JiraConnection, JiraIssueDetail, JiraMode,
    JiraSearch,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

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
    let alive = Rc::new(Cell::new(true));
    let own_dirty =
        Memo::new(move |_| original.get() != draft.get() || !key.get().is_empty() || clear.get());
    Effect::new(move |_| dirty.set(own_dirty.get()));
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
        searching.set(true);
        error.set(String::new());
        notice.set(String::new());
        results.set(None);
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
                        Ok(value) => detail.set(Some(value)),
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    view! {
        <section class="task-files" aria-labelledby="jira-heading" aria-busy=move || (loading.get() || pending.get() || searching.get() || inspecting.get()).to_string()>
            <h3 id="jira-heading">"Jira"</h3><p class="field-help">{move || i18n.text("Configure la connexion, puis lance une recherche explicite. Les résultats sont en lecture seule ; aucun ticket distant n’est modifié.")}</p>
            <p role="status" hidden=move || !loading.get()>{move || i18n.text("Chargement de la connexion Jira…")}</p>
            <form on:submit=save><fieldset disabled=move || !loaded.get() || loading.get() || pending.get() || searching.get() || inspecting.get()>
                <label for="jira-mode">{move || i18n.text("Version de Jira")}</label><select id="jira-mode" prop:value=move || if draft.get().mode==JiraMode::Cloud{"cloud"}else{"dataCenter"} on:change=move |event| draft.update(|draft| draft.mode=if event_target_value(&event)=="cloud"{JiraMode::Cloud}else{JiraMode::DataCenter})><option value="cloud">"Jira Cloud"</option><option value="dataCenter">"Jira Data Center"</option></select>
                <label for="jira-url">{move || i18n.text("Adresse de Jira")}</label><input id="jira-url" type="url" maxlength="2000" placeholder="https://tickets.example.test" prop:value=move || draft.get().url on:input=move |event| draft.update(|draft| draft.url=event_target_value(&event)) />
                <label for="jira-email">{move || i18n.text("E-mail (Cloud)")}</label><input id="jira-email" type="email" maxlength="300" prop:value=move || draft.get().email on:input=move |event| draft.update(|draft| draft.email=event_target_value(&event)) />
                <label for="jira-token">{move || i18n.text("Jeton API ou personnel")}</label><input id="jira-token" type="password" autocomplete="off" maxlength="4000" disabled=move || clear.get() prop:value=move || key.get() on:input=move |event| key.set(event_target_value(&event)) /><p class="field-help">{move || i18n.text(if token_set.get(){"Un jeton est enregistré. Laisser vide pour le conserver."}else{"Aucun jeton Jira enregistré."})}</p>
                <label><input id="jira-clear-token" type="checkbox" prop:checked=move || clear.get() on:change=move |event| {clear.set(event_target_checked(&event));if clear.get_untracked(){key.set(String::new());}} />{move || i18n.text("Retirer le jeton enregistré")}</label>
                <p class="field-help">{move || i18n.text("Changer l’adresse, le compte ou la version de Jira retire le jeton précédent si tu n’en fournis pas un nouveau.")}</p>
                <label for="jira-query">{move || i18n.text("Tickets à rechercher (JQL)")}</label><textarea id="jira-query" maxlength="4000" prop:value=move || draft.get().query on:input=move |event| draft.update(|draft| draft.query=event_target_value(&event)) />
            </fieldset><button id="save-jira" type="submit" disabled=move || !loaded.get() || pending.get() || loading.get() || searching.get() || inspecting.get() || !own_dirty.get()>{move || i18n.text("Enregistrer la connexion Jira")}</button></form>
            <div class="editor-actions"><button id="reload-jira" type="button" disabled=move || pending.get() || loading.get() || searching.get() || inspecting.get() on:click=move |_| {if own_dirty.get_untracked(){replace.set(true);}else{load.with_value(|load|load());}}>{move || i18n.text("Recharger la connexion Jira")}</button><button id="search-jira" type="button" disabled=move || !loaded.get() || own_dirty.get() || pending.get() || loading.get() || searching.get() || inspecting.get() || !token_set.get() || original.get().url.is_empty() on:click=search>{move || i18n.text("Rechercher les tickets Jira")}</button></div>
            <div class="confirmation" role="alert" hidden=move || !replace.get()><p>{move || i18n.text("Abandonner les modifications non enregistrées ?")}</p><button id="confirm-reload-jira" type="button" on:click=move |_| load.with_value(|load|load())>{move || i18n.text("Abandonner")}</button><button type="button" on:click=move |_|replace.set(false)>{move || i18n.text("Continuer à modifier")}</button></div>
            <p id="jira-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="jira-notice" role="status">{move || i18n.text(&notice.get())}</p>
            <p role="status" hidden=move || !searching.get()>{move || i18n.text("Recherche Jira en cours…")}</p>
            <p hidden=move || !results.with(|results| results.as_ref().is_some_and(|results| results.issues.is_empty()))>{move || i18n.text("Aucun ticket trouvé.")}</p>
            <p role="status" hidden=move || !inspecting.get()>{move || i18n.text("Chargement du ticket Jira…")}</p>
            {move || detail.get().map(|value| view!{<section aria-label=move || i18n.text("Détail du ticket Jira")><h4>{value.issue.key}" · "{value.issue.title}</h4><pre>{value.description}</pre><button type="button" on:click=move |_|detail.set(None)>{move || i18n.text("Fermer le détail")}</button></section>})}
            <ul class="service-endpoints"><For each=move || results.with(|results| results.as_ref().map(|results| results.issues.iter().skip(page.get()*200).take(200).cloned().collect::<Vec<_>>()).unwrap_or_default()) key=|issue| issue.id.clone() children=move |issue| {let id=issue.id.clone(); view!{<li class="jira-issue"><a href=issue.url target="_blank" rel="noopener noreferrer">{issue.key}" · "{issue.title}</a>" · "{issue.status}<button type="button" disabled=move || inspecting.get() || searching.get() || loading.get() || pending.get() || own_dirty.get() on:click=move |_|inspect.with_value(|inspect|inspect(id.clone()))>{move || i18n.text("Afficher le détail")}</button></li>}} /></ul>
            <div class="editor-actions" hidden=move || results.with(|results| results.as_ref().is_none_or(|results| results.issues.len()<=200))><button type="button" disabled=move || page.get()==0 on:click=move |_|page.update(|page|*page=page.saturating_sub(1))>{move || i18n.text("Précédent")}</button><button type="button" disabled=move || results.with(|results| results.as_ref().is_none_or(|results| (page.get()+1)*200>=results.issues.len())) on:click=move |_|page.update(|page|*page+=1)>{move || i18n.text("Suivant")}</button></div>
        </section>
    }
}
