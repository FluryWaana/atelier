use crate::{i18n::I18n, live::BoardState, write};
use atelier_core::{
    application::extensions::{
        self as protocol, Connection, Method, ModulePullRequestLink, RemoteOperation, Report,
    },
    contracts::Card,
};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ConnectorRepositories(
    connection: Signal<Option<Connection>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
    on_prepared: Callback<RemoteOperation>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let selected = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let target = RwSignal::new("main".to_owned());
    let draft = RwSignal::new(true);
    let touched = RwSignal::new(false);
    let report = RwSignal::new(None::<Report>);
    let frozen = RwSignal::new(None::<Card>);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let detach_approved = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let supported = Signal::derive(move || {
        connection.get().is_some_and(|connection| {
            connection
                .installed
                .manifest
                .methods
                .contains(&Method::RepositoriesPreview)
        })
    });
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == selected.get()).cloned())
    });
    let detach_snapshot =
        Memo::new(move |_| serde_json::to_string(&card.get()).unwrap_or_default());
    Effect::new(move |_| {
        let _ = detach_snapshot.get();
        detach_approved.set(false);
    });
    let key = Memo::new(move |_| {
        (
            selected.get(),
            connection.get().map(|connection| {
                (
                    connection.installed.manifest.id,
                    connection.installed.revision,
                )
            }),
        )
    });
    Effect::new(move |_| {
        let _ = key.get();
        if !touched.get_untracked() {
            if let Some(card) = card.get_untracked() {
                title.set(card.title.utf8().unwrap_or_default());
                body.set(
                    card.fields
                        .get::<String>("description")
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                );
                target.set(
                    card.fields
                        .get::<ModulePullRequestLink>("connectorPullRequest")
                        .ok()
                        .flatten()
                        .map(|link| link.pull_request.target_branch)
                        .or_else(|| {
                            card.fields
                                .get::<atelier_core::contracts::Worktree>("worktree")
                                .ok()
                                .flatten()
                                .map(|worktree| worktree.base)
                        })
                        .unwrap_or_else(|| "main".into()),
                );
            }
            report.set(None);
            frozen.set(None);
            error.set(String::new());
        }
    });
    Effect::new(move |_| {
        dirty.set(touched.get() || report.get().is_some() || detach_approved.get())
    });
    let stale = Memo::new(move |_| {
        let Some(expected) = frozen.get() else {
            return false;
        };
        card.get().is_none_or(|card| {
            serde_json::to_value(protocol::repository_source(&card)).ok()
                != serde_json::to_value(protocol::repository_source(&expected)).ok()
        })
    });
    let execute = StoredValue::new_local(move |method: Method| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let (Some(connection), Some(expected)) = (connection.get_untracked(), card.get_untracked())
        else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let profile = connection.installed;
        let publishing = method == Method::RepositoriesPublish;
        let (path, input) = if publishing {
            let Some(report) = report
                .get_untracked()
                .filter(|report| report.method == Method::RepositoriesPreview)
            else {
                return;
            };
            if stale.get_untracked() {
                return;
            }
            (
                format!("/api/extensions/{}/operations", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":"repositoriesPublish","input":{"expected":expected,"reportId":report.id}}),
            )
        } else {
            (
                format!("/api/extensions/{}/invoke", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":method,"action":null,"confirmed":true,"input":{"expected":expected,"target":target.get_untracked(),"title":title.get_untracked(),"body":body.get_untracked(),"draft":draft.get_untracked()}}),
            )
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        let expected_key = key.get_untracked();
        spawn_local(async move {
            let result = async {
                let response = write(&credential, Http::POST, &path, &input).await?;
                if publishing {
                    let operation = response
                        .json::<RemoteOperation>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                    Ok::<_, String>((None, Some(operation)))
                } else {
                    let report = response
                        .json::<Report>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                    Ok((Some(report), None))
                }
            }
            .await;
            if result.is_ok() {
                let _ = board.refresh(&credential).await;
            }
            if alive.get() {
                pending.set(false);
                if key.get_untracked() == expected_key {
                    match result {
                        Ok((next, operation)) => {
                            if let Some(operation) = operation {
                                on_prepared.run(operation);
                                report.set(None);
                                frozen.set(None);
                                touched.set(false);
                                notice.set("Aperçu d’envoi enregistré. Confirme son envoi dans le journal ci-dessous.".into());
                            } else {
                                report.set(next);
                                frozen.set(Some(expected));
                            }
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    let detach = move |_| {
        if !detach_approved.get_untracked() || pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let path = format!("/api/cards/{}/forge/detach", expected.id);
        let alive = mounted.get_value();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(
                &credential,
                Http::POST,
                &path,
                &serde_json::json!({"expected":expected,"confirmed":true}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                detach_approved.set(false);
                match result {
                    Ok(_) => {
                        report.set(None);
                        frozen.set(None);
                        touched.set(false);
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    view! {<fieldset hidden=move||!supported.get() disabled=move||pending.get()||blocked.get()>
        <legend>{move||i18n.text("PR/MR par ce module")}</legend>
        <p class="field-help">{move||i18n.text("Prépare un commit local et conserve le worktree propre. Le module gère son transport ; Atelier vérifie les fichiers, l’origine et les commits avant d’accepter le résultat.")}</p>
        <label for="connector-repository-card">{move||i18n.text("Tâche")}</label><select id="connector-repository-card" disabled=move||touched.get()||report.get().is_some() prop:value=move||selected.get() on:change=move|event|selected.set(event_target_value(&event))><option value="">{move||i18n.text("Choisir une tâche")}</option>{move||board.cards.get().into_iter().filter(|card|!card.fields.truthy("archived").unwrap_or(true)&&card.fields.raw("worktree").is_some()&&card.fields.raw("pullRequest").is_none()).map(|card|view!{<option value=card.id>{card.title.utf8().unwrap_or_default()}</option>}).collect_view()}</select>
        <label for="connector-repository-target">{move||i18n.text("Branche cible")}</label><input id="connector-repository-target" maxlength="180" prop:value=move||target.get() on:input=move|event|{target.set(event_target_value(&event));touched.set(true);report.set(None);}/>
        <label for="connector-repository-title">{move||i18n.text("Titre")}</label><input id="connector-repository-title" maxlength="255" prop:value=move||title.get() on:input=move|event|{title.set(event_target_value(&event));touched.set(true);report.set(None);}/>
        <label for="connector-repository-body">{move||i18n.text("Description")}</label><textarea id="connector-repository-body" maxlength="3500" prop:value=move||body.get() on:input=move|event|{body.set(event_target_value(&event));touched.set(true);report.set(None);}></textarea>
        <label><input type="checkbox" prop:checked=move||draft.get() on:change=move|event|{draft.set(event_target_checked(&event));touched.set(true);report.set(None);}/>{move||i18n.text("Brouillon")}</label>
        <button id="preview-connector-repository" type="button" disabled=move||card.get().is_none() on:click=move |_|execute.with_value(|execute|execute(Method::RepositoriesPreview))>{move||i18n.text("Préparer la publication de PR/MR")}</button>
        <button id="refresh-connector-repository" type="button" disabled=move||card.get().is_none_or(|card|card.fields.raw("connectorPullRequest").is_none()) on:click=move |_|execute.with_value(|execute|execute(Method::RepositoriesRefresh))>{move||i18n.text("Actualiser la PR/MR du module")}</button>
        <label hidden=move||card.get().is_none_or(|card|card.fields.raw("connectorPullRequest").is_none())><input type="checkbox" prop:checked=move||detach_approved.get() on:change=move|event|detach_approved.set(event_target_checked(&event))/>{move||i18n.text("Confirmer le retrait local de cette liaison de PR/MR")}</label><button type="button" disabled=move||!detach_approved.get() on:click=detach>{move||i18n.text("Retirer la liaison de PR/MR du module")}</button>
        {move||report.get().and_then(|report|protocol::repository_proposal(&report.output).ok()).map(|proposal|view!{<dl><dt>{i18n.text("Dépôt distant")}</dt><dd>{proposal.observation.reference.origin}</dd><dt>{i18n.text("Commit source")}</dt><dd>{proposal.context.source}</dd><dt>{i18n.text("Commit cible")}</dt><dd>{proposal.context.target}</dd><dt>{i18n.text("Base commune")}</dt><dd>{proposal.context.base}</dd></dl>})}
        <p role="status" hidden=move||!stale.get()>{move||i18n.text("La tâche a changé. Relis son aperçu avant de confirmer.")}</p>
        <button id="prepare-connector-repository" type="button" disabled=move||stale.get()||report.get().is_none_or(|report|report.method!=Method::RepositoriesPreview) on:click=move |_|execute.with_value(|execute|execute(Method::RepositoriesPublish))>{move||i18n.text("Enregistrer l’aperçu d’envoi de PR/MR")}</button>
        <button type="button" on:click=move |_|{report.set(None);frozen.set(None);touched.set(false);selected.set(String::new());notice.set(String::new());}>{move||i18n.text("Annuler le brouillon")}</button>
        <p class="field-help">{move||i18n.text("Après publication, lance la review depuis les exécutions de la tâche. Son résultat doit être revalidé sur les commits et les critères actuels.")}</p>
        <p role="status">{move||i18n.text(&notice.get())}</p><p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </fieldset>}
}
