use crate::{i18n::I18n, read, write};
use atelier_core::contracts::{
    ForgeConfiguration, ForgeConnections, ForgeKind, ForgeUpdate, PullRequest, PullRequestSummary,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn ForgeSettings(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let kind = RwSignal::new(ForgeKind::Github);
    let configurations = RwSignal::new(None::<ForgeConnections>);
    let original = RwSignal::new(String::new());
    let base = RwSignal::new(String::new());
    let secret = RwSignal::new(String::new());
    let clear = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let reading = RwSignal::new(false);
    let replace = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let number = RwSignal::new(String::new());
    let result = RwSignal::new(None::<PullRequest>);
    let listing = RwSignal::new(None::<Vec<PullRequestSummary>>);
    let page = RwSignal::new(0usize);
    let own_dirty =
        Memo::new(move |_| base.get() != original.get() || !secret.get().is_empty() || clear.get());
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let select = move |selected: ForgeKind| {
        let address = configurations.with_untracked(|configurations| {
            configurations
                .as_ref()
                .and_then(|configurations| {
                    configurations
                        .connections
                        .iter()
                        .find(|connection| connection.configuration.kind == selected)
                })
                .map(|connection| connection.configuration.base.clone())
                .unwrap_or_default()
        });
        kind.set(selected);
        original.set(address.clone());
        base.set(address);
        secret.set(String::new());
        clear.set(false);
        result.set(None);
        listing.set(None);
        page.set(0);
        replace.set(false);
    };
    let load_alive = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() || reading.get_untracked() || loading.get_untracked() {
            return;
        }
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
        let alive = load_alive.clone();
        spawn_local(async move {
            let fetched = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match fetched {
                    Ok(value) => {
                        configurations.set(Some(value));
                        select(kind.get_untracked());
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    Effect::new(move |_| load.with_value(|load| load()));
    let saving = alive.clone();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if pending.get_untracked()
            || loading.get_untracked()
            || reading.get_untracked()
            || !own_dirty.get_untracked()
        {
            return;
        }
        let Some(configuration) = configurations.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(ForgeUpdate {
            expected_revision: configuration.revision,
            configuration: ForgeConfiguration {
                kind: kind.get_untracked(),
                base: base.get_untracked(),
            },
            token: (!secret.get_untracked().is_empty()).then(|| secret.get_untracked()),
            clear_token: clear.get_untracked(),
        })
        .expect("forge configuration serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        result.set(None);
        listing.set(None);
        let alive = saving.clone();
        spawn_local(async move {
            let saved = async {
                write(&credential, Method::PUT, "/api/forges", &body)
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match saved {
                    Ok(value) => {
                        configurations.set(Some(value));
                        select(kind.get_untracked());
                        notice.set(
                            "Connexion de forge enregistrée. Aucun accès distant vérifié.".into(),
                        );
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let token_set = Memo::new(move |_| {
        configurations.with(|configurations| {
            configurations
                .as_ref()
                .and_then(|configurations| {
                    configurations
                        .connections
                        .iter()
                        .find(|connection| connection.configuration.kind == kind.get())
                })
                .is_some_and(|connection| connection.token_set)
        })
    });
    let inspecting = alive.clone();
    let inspect = StoredValue::new_local(move |list: bool| {
        if pending.get_untracked()
            || reading.get_untracked()
            || loading.get_untracked()
            || own_dirty.get_untracked()
        {
            return;
        }
        let Some(configuration) = configurations.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if !list
            && number
                .get_untracked()
                .parse::<u64>()
                .ok()
                .is_none_or(|number| number == 0 || number > i32::MAX as u64)
        {
            error.set("Numéro de PR/MR entier positif requis.".into());
            return;
        }
        let params = match web_sys::UrlSearchParams::new() {
            Ok(params) => params,
            Err(_) => {
                error.set("Paramètres de lecture invalides.".into());
                return;
            }
        };
        params.set("revision", &configuration.revision.to_string());
        params.set("kind", kind.get_untracked().name());
        params.set("repository", &repository.get_untracked());
        if !list {
            params.set("number", &number.get_untracked());
        }
        let path = format!(
            "/api/forges/{}?{}",
            if list {
                "pull-requests"
            } else {
                "pull-request"
            },
            String::from(params.to_string())
        );
        reading.set(true);
        error.set(String::new());
        result.set(None);
        if list {
            listing.set(None);
            page.set(0);
        }
        let alive = inspecting.clone();
        spawn_local(async move {
            let fetched = async {
                let response = read(&credential, &path).await?;
                if list {
                    let values = response
                        .json::<Vec<PullRequestSummary>>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                    Ok::<_, String>((Some(values), None))
                } else {
                    let value = response
                        .json::<PullRequest>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                    Ok((None, Some(value)))
                }
            }
            .await;
            if alive.get() {
                reading.set(false);
                match fetched {
                    Ok((values, value)) => {
                        if let Some(values) = values {
                            listing.set(Some(values));
                        }
                        result.set(value);
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    view! {<section class="task-files" aria-labelledby="forges-heading" aria-busy=move||(pending.get()||loading.get()||reading.get()).to_string()><h3 id="forges-heading">{move||i18n.text("Connexions aux forges")}</h3>
        <p class="field-help">{move||i18n.text("Configure GitHub, GitLab ou Azure DevOps Services, puis lis explicitement une PR/MR. Cette lecture ne crée ni publication, ni commentaire, ni exécution de CI.")}</p>
        <label for="forge-kind">{move||i18n.text("Forge")}</label><select id="forge-kind" prop:value=move||kind.get().name() disabled=move||pending.get()||loading.get()||reading.get()||own_dirty.get() on:change=move|event|{let selected=match event_target_value(&event).as_str(){"gitlab"=>ForgeKind::Gitlab,"azure"=>ForgeKind::Azure,_=>ForgeKind::Github};repository.set(String::new());number.set(String::new());select(selected);} ><option value="github">"GitHub"</option><option value="gitlab">"GitLab"</option><option value="azure">"Azure DevOps Services"</option></select>
        <form on:submit=save><fieldset disabled=move||configurations.with(|configurations|configurations.is_none())||pending.get()||loading.get()||reading.get()><label for="forge-base">{move||i18n.text("Adresse du site ou de l’organisation")}</label><input id="forge-base" type="url" required maxlength="2000" placeholder=move||if kind.get()==ForgeKind::Azure{"https://dev.azure.com/ORGANISATION"}else{"https://forge.example.test"} prop:value=move||base.get() on:input=move|event|base.set(event_target_value(&event)) />
            <p class="field-help">{move||i18n.text("Utilise la racine du site GitHub, le préfixe du site GitLab ou l’adresse de l’organisation Azure. Changer l’adresse retire le jeton précédent si tu n’en fournis pas un nouveau.")}</p><label for="forge-token">{move||i18n.text("Jeton personnel de la forge")}</label><input id="forge-token" type="password" autocomplete="off" maxlength="4000" disabled=move||clear.get() prop:value=move||secret.get() on:input=move|event|secret.set(event_target_value(&event)) /><p class="field-help">{move||i18n.text(if token_set.get(){"Un jeton est enregistré. Laisser vide pour le conserver."}else{"Aucun jeton enregistré pour cette forge."})}</p><label><input id="forge-clear-token" type="checkbox" prop:checked=move||clear.get() on:change=move|event|{clear.set(event_target_checked(&event));if clear.get_untracked(){secret.set(String::new());}} />{move||i18n.text("Retirer le jeton enregistré")}</label><button id="save-forge" type="submit" disabled=move||!own_dirty.get()>{move||i18n.text("Enregistrer la connexion de forge")}</button></fieldset></form>
        <button id="reload-forges" type="button" disabled=move||pending.get()||loading.get()||reading.get() on:click=move|_|{if own_dirty.get_untracked(){replace.set(true);}else{load.with_value(|load|load());}}>{move||i18n.text("Recharger les connexions aux forges")}</button><div class="confirmation" hidden=move||!replace.get()><p>{move||i18n.text("Abandonner les modifications non enregistrées ?")}</p><button id="confirm-reload-forges" type="button" on:click=move|_|load.with_value(|load|load())>{move||i18n.text("Abandonner")}</button><button type="button" on:click=move|_|replace.set(false)>{move||i18n.text("Continuer à modifier")}</button></div>
        <form on:submit=move|event:leptos::ev::SubmitEvent|{event.prevent_default();inspect.with_value(|inspect|inspect(false));}><fieldset disabled=move||!token_set.get()||pending.get()||loading.get()||reading.get()||own_dirty.get()><label for="forge-repository">{move||i18n.text("Dépôt de la PR/MR")}</label><input id="forge-repository" required maxlength="1000" placeholder=move||i18n.text(if kind.get()==ForgeKind::Azure{"projet/dépôt"}else{"groupe/dépôt"}) prop:value=move||repository.get() on:input=move|event|{repository.set(event_target_value(&event));listing.set(None);result.set(None);} /><button id="list-forge-prs" type="button" disabled=move||repository.get().trim().is_empty() on:click=move|_|inspect.with_value(|inspect|inspect(true))>{move||i18n.text("Lister les PR/MR ouvertes")}</button><label for="forge-pr-number">{move||i18n.text("Numéro de PR/MR")}</label><input id="forge-pr-number" type="number" min="1" max="2147483647" step="1" required prop:value=move||number.get() on:input=move|event|number.set(event_target_value(&event)) /><button id="read-forge-pr" type="submit">{move||i18n.text("Lire la PR/MR")}</button></fieldset></form>
        <section hidden=move||listing.get().is_none() aria-labelledby="forge-list-heading"><h4 id="forge-list-heading">{move||i18n.text("PR/MR ouvertes du dépôt")}</h4><p class="field-help">{move||i18n.text("Liste lue sur demande. Sélectionne un numéro, puis lis la PR/MR pour vérifier ses commits. Aucun résultat partiel n’est présenté comme une liste complète.")}</p><p hidden=move||listing.with(|listing|listing.as_ref().is_none_or(|items|!items.is_empty()))>{move||i18n.text("Aucune PR/MR ouverte dans ce dépôt.")}</p>
        <For each={move||listing.get().unwrap_or_default().into_iter().skip(page.get()*100).take(100).collect::<Vec<_>>()} key=|pr|pr.number children=move|pr|{let id=pr.number;view!{<article><button class="forge-list-choice" type="button" disabled=move||reading.get()||pending.get() on:click=move|_|{number.set(id.to_string());notice.set("PR/MR sélectionnée. Lance sa lecture pour obtenir ses détails.".into());}>{format!("#{} · {}",pr.number,pr.title)}</button><p>{pr.source_branch}{" → "}{pr.target_branch}</p><p hidden=!pr.draft>{move||i18n.text("Brouillon")}</p></article>}} />
        <div class="editor-actions"><button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled=move||listing.with(|listing|listing.as_ref().is_none_or(|items|(page.get()+1)*100>=items.len())) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button></div></section>
        <p role="status" hidden=move||!reading.get()>{move||i18n.text("Lecture de la PR/MR…")}</p><p class="error" id="forge-error" role="alert">{move||i18n.text(&error.get())}</p><p id="forge-notice" role="status">{move||i18n.text(&notice.get())}</p>
        {move||result.get().map(|pr|view!{<article class="task-files"><h4><a href=pr.url target="_blank" rel="noopener noreferrer">{format!("#{} · {}",pr.number,pr.title)}</a></h4><p>{pr.repository}" · "{pr.source_branch}" → "{pr.target_branch}</p><p>{move||i18n.text(match pr.status.as_str(){"active"=>"PR/MR ouverte","completed"=>"PR/MR fusionnée","abandoned"=>"PR/MR fermée",_=>"État de PR/MR inconnu"})}</p><p hidden=!pr.draft>{move||i18n.text("Brouillon")}</p><p hidden=!pr.fork>{move||i18n.text("Cette PR/MR provient d’un fork. Sa lecture ne l’autorise pas pour une review ou une publication.")}</p><dl><dt>{move||i18n.text("Commit source déclaré")}</dt><dd>{pr.source_commit.unwrap_or_else(||i18n.text("Indisponible"))}</dd><dt>{move||i18n.text("Commit cible déclaré")}</dt><dd>{pr.target_commit.unwrap_or_else(||i18n.text("Indisponible"))}</dd></dl></article>})}
    </section>}
}
