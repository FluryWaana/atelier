use crate::{i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    application::extensions::{
        self as protocol, Connection, Method, Registry, RemoteOperation, Report,
    },
    contracts::Card,
};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ConnectorTicketTransitions(
    card_id: String,
    card: Signal<Option<Card>>,
    module: Signal<String>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let connection = RwSignal::new(None::<Connection>);
    let preview = RwSignal::new(None::<(Card, Report)>);
    let selected = RwSignal::new(String::new());
    let journal_dirty = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let key = Memo::new(move |_| {
        serde_json::to_string(
            &card
                .get()
                .map(|card| atelier_core::application::jira::card_snapshot(&card)),
        )
        .unwrap_or_default()
    });
    let module_key = Memo::new(move |_| module.get());
    Effect::new(move |_| {
        let _ = key.get();
        preview.set(None);
        selected.set(String::new());
        notice.set(false);
    });
    Effect::new(move |_| {
        let _ = module_key.get();
        connection.set(None);
        preview.set(None);
        selected.set(String::new());
    });
    Effect::new(move |_| dirty.set(preview.get().is_some() || journal_dirty.get()));
    let execute = StoredValue::new_local(move |prepare: bool| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let module = module.get_untracked();
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let chosen = selected.get_untracked();
        let snapshot = preview.get_untracked();
        if prepare && (chosen.is_empty() || snapshot.is_none()) {
            return;
        }
        let expected_key = key.get_untracked();
        let alive = alive.clone();
        pending.set(true);
        error.set(String::new());
        notice.set(false);
        spawn_local(async move {
            let result=async {
                let registry=read(&credential,"/api/extensions").await?.json::<Registry>().await.map_err(|_|"Réponse du serveur invalide.".to_owned())?;
                let profile=registry.modules.into_iter().find(|connection|connection.installed.manifest.id==module).ok_or("Réinstalle le connecteur de ce ticket avant de lire ses transitions.")?;
                if !registry.native_available || ![Method::TicketsTransitions,Method::TicketsTransition,Method::Reconcile].iter().all(|method|profile.installed.manifest.methods.contains(method)) {
                    return Err("Ce connecteur ne propose pas les transitions de tickets.".to_owned());
                }
                let revision=profile.installed.revision;
                let (path,body)=if prepare {
                    let (shown,report)=snapshot.ok_or("Relis les transitions du ticket avant de confirmer.")?;
                    let choices=protocol::ticket_transitions(&report.output).map_err(str::to_owned)?;
                    if !choices.transitions.iter().any(|transition|transition.id==chosen&&!transition.requires_fields){return Err("Choisis une transition disponible sans champ supplémentaire.".into());}
                    (format!("/api/extensions/{module}/operations"),serde_json::json!({"expectedRevision":revision,"method":"ticketsTransition","input":{"expected":atelier_core::application::jira::card_snapshot(&shown),"reportId":report.id,"transitionId":chosen}}))
                }else{
                    (format!("/api/extensions/{module}/invoke"),serde_json::json!({"expectedRevision":revision,"method":"ticketsTransitions","action":null,"input":{"expected":atelier_core::application::jira::card_snapshot(&expected)},"confirmed":true}))
                };
                let response=write(&credential,Http::POST,&path,&body).await?;
                let report=if prepare {response.json::<RemoteOperation>().await.map_err(|_|"Réponse du serveur invalide.".to_owned())?;None}
                    else {let report=response.json::<Report>().await.map_err(|_|"Réponse du serveur invalide.".to_owned())?;protocol::ticket_transitions(&report.output).map_err(str::to_owned)?;Some(report)};
                Ok::<_,String>((profile,report))
            }.await;
            if alive.get() {
                pending.set(false);
                if key.get_untracked() == expected_key {
                    match result {
                        Ok((profile, Some(report))) => {
                            connection.set(Some(profile));
                            preview.set(Some((expected, report)));
                            selected.set(String::new());
                        }
                        Ok((profile, None)) => {
                            connection.set(Some(profile));
                            preview.set(None);
                            selected.set(String::new());
                            notice.set(true);
                        }
                        Err(message) => error.set(message),
                    }
                } else {
                    error
                        .set("La carte a changé pendant la lecture. Relis ses transitions.".into());
                }
            }
        });
    });
    view! {<div class="task-files">
        <h4>{move||i18n.text("Transitions du ticket de connecteur")}</h4>
        <button id="read-connector-transitions" type="button" disabled=move||pending.get()||blocked.get()||journal_dirty.get() on:click=move |_|execute.with_value(|execute|execute(false))>{move||i18n.text("Lire les transitions disponibles")}</button>
        {move||preview.get().and_then(|(_,report)|protocol::ticket_transitions(&report.output).ok()).map(|report|view!{
            <p>{i18n.text("Statut distant de départ")}" : "{report.status}</p>
            <p hidden=!report.transitions.is_empty()>{i18n.text("Aucune transition disponible pour ce ticket.")}</p>
            <label for="connector-ticket-transition">{i18n.text("Transition")}</label>
            <select id="connector-ticket-transition" disabled=move||pending.get()||blocked.get() prop:value=move||selected.get() on:change=move|event|selected.set(event_target_value(&event))>
                <option value="">{i18n.text("Choisir une transition")}</option>
                {report.transitions.into_iter().map(|transition|view!{<option value=transition.id disabled=transition.requires_fields>{format!("{} → {}{}",transition.name,transition.status,if transition.requires_fields {i18n.text(" — champs supplémentaires requis")}else{String::new()})}</option>}).collect_view()}
            </select>
            <button id="prepare-connector-transition" type="button" disabled=move||pending.get()||blocked.get()||selected.get().is_empty() on:click=move |_|execute.with_value(|execute|execute(true))>{move||i18n.text("Préparer cette transition dans le journal")}</button>
            <button type="button" disabled=move||pending.get() on:click=move |_|{preview.set(None);selected.set(String::new());}>{move||i18n.text("Annuler l’aperçu")}</button>
        })}
        <p hidden=move||!notice.get() role="status">{move||i18n.text("Transition préparée. Vérifie le ticket et les statuts dans le journal puis confirme l’envoi séparément.")}</p>
        <p role="alert" class="error">{move||i18n.text(&error.get())}</p>
        <crate::connector_effects::ConnectorEffects connection=Signal::derive(move||connection.get()) board token pending dirty=journal_dirty blocked=Memo::new(move |_|blocked.get()||preview.get().is_some()) card_id/>
    </div>}
}
