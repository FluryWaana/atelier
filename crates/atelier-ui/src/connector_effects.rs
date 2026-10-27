use crate::{connectors::translated, i18n::I18n, read, write};
use atelier_core::{
    application::extensions::{
        self as protocol, Connection, Effect as RemoteEffect, Method, Registry,
        RemoteOperation as Operation,
    },
    contracts::ForgeWriteState as Phase,
};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[path = "connector_repositories.rs"]
mod connector_repositories;
#[path = "connector_ticket_reset.rs"]
mod connector_ticket_reset;
#[path = "connector_time.rs"]
mod connector_time;
fn state(phase: Phase) -> &'static str {
    match phase {
        Phase::Preparing => "Aperçu non envoyé",
        Phase::Sending => "Envoi en cours",
        Phase::Uncertain => "Effet distant incertain",
        Phase::Confirmed => "Confirmé par le module",
        Phase::Reconciled => "Rapproché par le module",
        Phase::Failed => "Annulé ou déclaré non envoyé",
    }
}
#[component]
pub fn ConnectorEffects(
    connection: Signal<Option<Connection>>,
    board: crate::live::BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
    #[prop(optional)] card_id: String,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let operations = RwSignal::new(Vec::<Operation>::new());
    let loading = RwSignal::new(false);
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let ticket_kind = RwSignal::new(String::new());
    let ticket_title = RwSignal::new(String::new());
    let ticket_description = RwSignal::new(String::new());
    let time_dirty = RwSignal::new(false);
    let repository_dirty = RwSignal::new(false);
    let reset_dirty = RwSignal::new(false);
    let scoped = !card_id.is_empty();
    let heading = if scoped {
        format!("connector-effects-heading-{card_id}")
    } else {
        "connector-effects-heading".into()
    };
    let reload_id = if scoped {
        format!("reload-connector-effects-{card_id}")
    } else {
        "reload-connector-effects".into()
    };
    let card_id = StoredValue::new(card_id);
    let page = RwSignal::new(0usize);
    let generation = RwSignal::new(0u64);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let context = Memo::new(move |_| {
        connection.get().map(|connection| {
            (
                connection.installed.manifest.id,
                connection.installed.revision,
            )
        })
    });
    Effect::new(move |_| {
        dirty.set(
            !approved.get().is_empty()
                || time_dirty.get()
                || repository_dirty.get()
                || reset_dirty.get()
                || !ticket_kind.get().is_empty()
                || !ticket_title.get().is_empty()
                || !ticket_description.get().is_empty()
                || operations.with(|operations| {
                    operations
                        .iter()
                        .any(|operation| operation.state == Phase::Preparing)
                }),
        )
    });
    Effect::new(move |_| {
        let _ = context.get();
        approved.set(String::new());
        operations.set(Vec::new());
        error.set(String::new());
    });
    let reading = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() {
            return;
        }
        let Some(expected) = context.get_untracked() else {
            operations.set(Vec::new());
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        loading.set(true);
        generation.update(|value| *value = value.wrapping_add(1));
        let reading_generation = generation.get_untracked();
        approved.set(String::new());
        let alive = reading.clone();
        let expected_again = expected.clone();
        spawn_local(async move {
            let result = async {
                read(
                    &credential,
                    &format!("/api/extensions/{}/operations", expected.0),
                )
                .await?
                .json::<Vec<Operation>>()
                .await
                .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() && generation.get_untracked() == reading_generation {
                loading.set(false);
                if context.get_untracked() == Some(expected_again) {
                    match result {
                        Ok(mut values) => {
                            let selected = card_id.get_value();
                            if !selected.is_empty() {
                                values.retain(|operation| {
                                    (operation.method == Method::TicketsTransition
                                        && operation.input["cardId"].as_str()
                                            == Some(selected.as_str()))
                                        || (operation.method == Method::RepositoriesPublish
                                            && operation.input["context"]["cardId"].as_str()
                                                == Some(selected.as_str()))
                                });
                            }
                            values.reverse();
                            operations.set(values);
                            page.set(0);
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    Effect::new(move |_| {
        let _ = context.get();
        if !pending.get() {
            load.with_value(|load| load());
        }
    });
    let writing = alive.clone();
    let execute = StoredValue::new_local(
        move |(action, operation): (String, Option<Operation>)| {
            if pending.get_untracked()
                || loading.get_untracked()
                || (blocked.get_untracked()
                    && !(operation.is_some() && matches!(action.as_str(), "reconcile" | "cancel")))
            {
                return;
            }
            let Some(connection) = connection.get_untracked() else {
                return;
            };
            let reconciling = action == "reconcile" && operation.is_some();
            let profile = connection.installed;
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            let (path, body) = if let Some(operation) = operation {
                let key = format!(
                    "{}:{}:{}",
                    operation.id, operation.version, profile.revision
                );
                if operation.module != profile.manifest.id
                    || (action != "cancel" && approved.get_untracked() != key)
                {
                    return;
                }
                (
                    format!(
                        "/api/extensions/{}/operations/{}/{action}",
                        profile.manifest.id, operation.id
                    ),
                    serde_json::json!({"expectedVersion":operation.version,"expectedRevision":profile.revision,"confirmed":true}),
                )
            } else {
                (
                    format!("/api/extensions/{}/operations", profile.manifest.id),
                    if action == "@create-ticket" {
                        serde_json::json!({"expectedRevision":profile.revision,"method":"ticketsCreate","input":{"kind":ticket_kind.get_untracked(),"title":ticket_title.get_untracked(),"description":ticket_description.get_untracked()}})
                    } else {
                        serde_json::json!({"expectedRevision":profile.revision,"action":action})
                    },
                )
            };
            pending.set(true);
            approved.set(String::new());
            error.set(String::new());
            let alive = writing.clone();
            let module = profile.manifest.id;
            spawn_local(async move {
                let result = async {
                    let mut body = body;
                    if reconciling {
                        let registry = read(&credential, "/api/extensions")
                            .await?
                            .json::<Registry>()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        let current = registry
                            .modules
                            .into_iter()
                            .find(|connection| connection.installed.manifest.id == module)
                            .ok_or("Réinstalle ce module pour rapprocher son opération.")?;
                        body["expectedRevision"] = serde_json::json!(current.installed.revision);
                    }
                    write(&credential, Http::POST, &path, &body)
                        .await?
                        .json::<Operation>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                let _ = board.refresh(&credential).await;
                if alive.get() {
                    pending.set(false);
                    match result {
                        Ok(operation) => {
                            if operation.method == Method::TicketsCreate
                                && operation.state == Phase::Preparing
                            {
                                ticket_kind.set(String::new());
                                ticket_title.set(String::new());
                                ticket_description.set(String::new());
                            }
                            operations.update(|operations| {
                                if let Some(existing) = operations
                                    .iter_mut()
                                    .find(|existing| existing.id == operation.id)
                                {
                                    *existing = operation;
                                } else {
                                    operations.insert(0, operation);
                                }
                            });
                            page.set(0);
                        }
                        Err(message) => {
                            error.set(message);
                            load.with_value(|load| load());
                        }
                    }
                }
            });
        },
    );
    let heading_id = heading.clone();
    view! {<section class="task-files" data-connector-journal-card=card_id.get_value() hidden=move||connection.get().is_none() aria-labelledby=heading aria-busy=move||(loading.get()||pending.get()).to_string()>
        <h4 id=heading_id>{move||i18n.text("Écritures distantes du connecteur")}</h4>
        <Show when=move||!scoped>
        <connector_time::ConnectorTime connection board token pending dirty=time_dirty blocked=Memo::new(move |_|blocked.get()||loading.get()||operations.with(|operations|operations.iter().any(|operation|matches!(operation.state,Phase::Preparing|Phase::Sending|Phase::Uncertain))))/>
        <connector_ticket_reset::ConnectorTicketReset connection board token pending dirty=reset_dirty blocked=Memo::new(move |_|blocked.get()||loading.get()||operations.with(|operations|operations.iter().any(|operation|matches!(operation.state,Phase::Preparing|Phase::Sending|Phase::Uncertain))))/>
        <connector_repositories::ConnectorRepositories connection board token pending dirty=repository_dirty blocked=Memo::new(move |_|blocked.get()||loading.get()||operations.with(|operations|operations.iter().any(|operation|matches!(operation.state,Phase::Preparing|Phase::Sending|Phase::Uncertain)))) on_prepared=Callback::new(move |operation:Operation|{operations.update(|rows|rows.insert(0,operation));page.set(0);})/>
        <fieldset hidden=move||connection.get().is_none_or(|connection|!connection.installed.manifest.methods.contains(&Method::TicketsCreate)) disabled=move||pending.get()||loading.get()||blocked.get()||operations.with(|rows|rows.iter().any(|row|matches!(row.state,Phase::Preparing|Phase::Sending|Phase::Uncertain)))>
            <legend>{move||i18n.text("Créer un ticket distant")}</legend>
            <label for="connector-ticket-kind">{move||i18n.text("Type de ticket")}</label><input id="connector-ticket-kind" maxlength="100" prop:value=move||ticket_kind.get() on:input=move|event|ticket_kind.set(event_target_value(&event))/>
            <label for="connector-ticket-title">{move||i18n.text("Titre")}</label><input id="connector-ticket-title" maxlength="255" prop:value=move||ticket_title.get() on:input=move|event|ticket_title.set(event_target_value(&event))/>
            <label for="connector-ticket-description">{move||i18n.text("Description")}</label><textarea id="connector-ticket-description" maxlength="20000" prop:value=move||ticket_description.get() on:input=move|event|ticket_description.set(event_target_value(&event))></textarea>
            <button type="button" disabled=move||ticket_kind.get().trim().is_empty()||ticket_title.get().trim().is_empty() on:click=move |_|execute.with_value(|execute|execute(("@create-ticket".into(),None)))>{move||i18n.text("Préparer la création du ticket")}</button>
        </fieldset>
        <p class="field-help">{move||i18n.text("Prépare une action déclarée, vérifie sa portée puis confirme son envoi unique. Une réponse perdue impose un rapprochement en lecture seule. L’aperçu réserve l’application jusqu’à son envoi ou son annulation.")}</p>
        <div><For each=move||connection.get().map(|connection|connection.installed.manifest.tabs.into_iter().flat_map(|tab|tab.actions).filter(|action|action.effect==RemoteEffect::Remote).collect::<Vec<_>>()).unwrap_or_default() key=|action|action.id.clone() children=move|action|{let selected=StoredValue::new(action.id);let label=action.label;view!{<button class="prepare-connector-effect" type="button" disabled=move||pending.get()||loading.get()||blocked.get()||operations.with(|operations|operations.iter().any(|operation|matches!(operation.state,Phase::Preparing|Phase::Sending|Phase::Uncertain))) on:click=move |_|execute.with_value(|execute|execute((selected.get_value(),None)))>{move||connection.get().map(|connection|translated(i18n,&connection.installed.manifest,&label))}</button>}}/></div>
        </Show>
        <button id=reload_id type="button" disabled=move||pending.get()||loading.get() on:click=move |_|{error.set(String::new());load.with_value(|load|load());}>{move||i18n.text("Relire l’historique des opérations")}</button>
        <p hidden=move||loading.get()||!operations.with(Vec::is_empty)>{move||i18n.text("Aucune opération enregistrée pour ce connecteur.")}</p>
        <For each=move||{let _=i18n.code();operations.with(|operations|operations.iter().skip(page.get()*20).take(20).cloned().collect::<Vec<_>>())} key=move|operation|(operation.id.clone(),operation.version,i18n.code()) children=move|operation|{
            let current=StoredValue::new(operation.clone());
            let key=move||context.get().map(|(_,revision)|format!("{}:{}:{revision}",current.with_value(|operation|operation.id.clone()),current.with_value(|operation|operation.version))).unwrap_or_default();
            let label=if operation.method==Method::RepositoriesPublish{i18n.text("Publier une PR/MR par ce module")}else if operation.method==Method::TicketsTransition{i18n.text("Modifier le statut d’un ticket distant")}else if operation.method==Method::TimeSend {i18n.text("Exporter une saisie de temps")}else if operation.method==Method::TicketsCreate {i18n.text("Créer un ticket distant")}else{operation.manifest.tabs.iter().flat_map(|tab|&tab.actions).find(|action|action.id==operation.action).map(|action|action.label.clone()).unwrap_or_default()};
            let report=operation.reply.as_ref().map(|reply|protocol::Report{id:operation.id.clone(),method:operation.method,action:if operation.method==Method::Action {Some(operation.action.clone())}else{None},message:reply.message.clone(),output:reply.output.clone(),observed_at:operation.updated_at.clone()});
            let items=report.as_ref().map(protocol::report_items).transpose().unwrap_or_default().unwrap_or_default();
            view!{<article class="task-files" data-connector-operation=operation.id.clone()>
                <h5>{translated(i18n,&operation.manifest,&label)}</h5><p class="connector-effect-state">{i18n.text(state(operation.state))}</p><code>{operation.id.clone()}</code><p>{operation.updated_at.clone()}</p>
                <p class="field-help">{translated(i18n,&operation.manifest,&operation.manifest.name)}</p>
                <p hidden=operation.method!=Method::Action>{i18n.text("Tickets locaux dans cette portée : {0}").replace("{0}",&operation.input["importedCount"].as_u64().unwrap_or(0).to_string())}</p>
                {if operation.method==Method::TicketsTransition{protocol::ticket_transition_payload(&operation.input).ok().map(|payload|view!{<dl>
                    <dt>{i18n.text("Tâche")}</dt><dd>{payload.card_title}</dd><dt>{i18n.text("Ticket")}</dt><dd>{payload.ticket_key}</dd><dt>{i18n.text("Instance")}</dt><dd>{payload.ticket.instance}</dd>
                    <dt>{i18n.text("Statut distant de départ")}</dt><dd>{payload.from_status}</dd><dt>{i18n.text("Transition")}</dt><dd>{payload.transition.name}</dd><dt>{i18n.text("Statut distant demandé")}</dt><dd>{payload.transition.status}</dd>
                    <dt>{i18n.text("Limite de concurrence distante")}</dt><dd>{i18n.text("Un autre acteur peut modifier le ticket entre la relecture et l’envoi. Le rapprochement constate la déclaration du module, sans attribuer l’effet à Atelier.")}</dd>
                </dl>})}else{None}}
                {if operation.method==Method::TicketsCreate {Some(view!{<dl><dt>{i18n.text("Type de ticket")}</dt><dd>{operation.input["kind"].as_str().unwrap_or_default().to_owned()}</dd><dt>{i18n.text("Titre")}</dt><dd>{operation.input["title"].as_str().unwrap_or_default().to_owned()}</dd><dt>{i18n.text("Description")}</dt><dd class="pre-wrap">{operation.input["description"].as_str().unwrap_or_default().to_owned()}</dd></dl>})}else{None}}
                {if operation.method==Method::RepositoriesPublish{protocol::repository_proposal(&operation.input).ok().map(|proposal|view!{<dl><dt>{i18n.text("Dépôt distant")}</dt><dd>{proposal.observation.reference.origin}</dd><dt>{i18n.text("Titre")}</dt><dd>{proposal.title}</dd><dt>{i18n.text("Description")}</dt><dd class="pre-wrap">{proposal.body}</dd><dt>{i18n.text("Commit source")}</dt><dd>{proposal.context.source}</dd><dt>{i18n.text("Commit cible")}</dt><dd>{proposal.context.target}</dd><dt>{i18n.text("Branche cible")}</dt><dd>{proposal.observation.target_branch}</dd></dl>})}else{None}}
                {if operation.method==Method::TimeSend {protocol::time_proposal(&operation.input).ok().map(|proposal|view!{<dl>
                    <dt>{i18n.text("Tâche")}</dt><dd>{proposal.source.card_title}</dd><dt>{i18n.text("Saisie de temps")}</dt><dd>{proposal.source.id}</dd>
                    <dt>{i18n.text("Destination")}</dt><dd>{proposal.instance}</dd><dt>{i18n.text("Compte")}</dt><dd>{proposal.account}</dd><dt>{i18n.text("Référence distante")}</dt><dd>{proposal.reference}</dd>
                    {proposal.source.ticket.map(|ticket|view!{<dt>{i18n.text("Ticket source")}</dt><dd>{format!("{} · {}",ticket.key,ticket.instance)}</dd>})}
                    <dt>{i18n.text("Date")}</dt><dd>{format!("{} {}",proposal.source.date,proposal.source.start_time)}</dd><dt>{i18n.text("Durée")}</dt><dd>{format!("{} s",proposal.source.seconds)}</dd>
                    <dt>{i18n.text("Description")}</dt><dd class="pre-wrap">{proposal.source.description}</dd></dl>})}else{None}}
                <dl>{operation.manifest.fields.iter().filter(|field|field.kind!=protocol::FieldKind::Password).filter_map(|field|operation.values.get(&field.id).map(|value|(field,value))).map(|(field,value)|view!{<dt>{translated(i18n,&operation.manifest,&field.label)}</dt><dd>{value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string())}</dd>}).collect_view()}</dl>
                <p>{operation.reply.as_ref().map(|reply|translated(i18n,&operation.manifest,&reply.message)).unwrap_or_default()}</p>
                {operation.reply.as_ref().and_then(|reply|protocol::remote_receipt(&reply.output,&operation.id).ok().flatten()).map(|receipt|view!{<p>{i18n.text(match receipt.outcome{protocol::RemoteOutcome::Applied=>"Effet déclaré appliqué",protocol::RemoteOutcome::NotAttempted=>"Envoi déclaré non tenté",protocol::RemoteOutcome::Unknown=>"Résultat déclaré inconnu"})}" · "{i18n.text("Référence déclarée par le module : ")}<code>{receipt.reference}</code>" · "{receipt.observed_at}</p>})}
                <dl>{items.into_iter().map(|item|view!{<dt>{translated(i18n,&operation.manifest,&item.label)}</dt><dd>{item.value}</dd>}).collect_view()}</dl>
                <div hidden=!matches!(operation.state,Phase::Preparing|Phase::Uncertain)><label><input class="confirm-connector-effect" type="checkbox" disabled=move||pending.get()||loading.get()||(blocked.get()&&current.with_value(|operation|operation.state)!=Phase::Uncertain) prop:checked=move||approved.get()==key() on:change=move|event|approved.set(if event_target_checked(&event){key()}else{String::new()})/>{move||i18n.text(if current.with_value(|operation|operation.state)==Phase::Uncertain{"Rapprocher cette opération sans renvoyer son action."}else{"Envoyer une seule fois cette action dans la portée affichée."})}</label>
                    <button class="send-connector-effect" type="button" disabled=move||pending.get()||loading.get()||(blocked.get()&&current.with_value(|operation|operation.state)!=Phase::Uncertain)||approved.get()!=key() on:click=move |_|execute.with_value(|execute|execute(((if current.with_value(|operation|operation.state)==Phase::Uncertain{"reconcile"}else{"send"}).into(),Some(current.get_value()))))>{move||i18n.text(if current.with_value(|operation|operation.state)==Phase::Uncertain{"Rapprocher l’effet distant"}else{"Confirmer l’envoi unique"})}</button>
                    <button class="cancel-connector-effect" hidden=operation.state!=Phase::Preparing type="button" disabled=move||pending.get()||loading.get() on:click=move |_|execute.with_value(|execute|execute(("cancel".into(),Some(current.get_value()))))>{move||i18n.text("Annuler cet aperçu non envoyé")}</button>
                </div>
            </article>}
        }/>
        <button type="button" disabled=move||page.get()==0 on:click=move |_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled={move||(page.get()+1)*20>=operations.with(Vec::len)} on:click=move |_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
