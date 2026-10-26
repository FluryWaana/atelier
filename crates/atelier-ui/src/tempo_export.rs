use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    contracts::{TempoConnection, TempoExportIntent, TempoExportPreview, TempoExportRequest},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, collections::BTreeSet, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn TempoExport(
    entry: Signal<Document>,
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    drafts: RwSignal<BTreeSet<String>>,
    archived: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card_id = StoredValue::new(card_id);
    let id = StoredValue::new(field(&entry.get_untracked(), "id"));
    let card = Signal::derive(move || {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == card_id.get_value())
                .cloned()
        })
    });
    let preview = Signal::derive(move || {
        entry
            .get()
            .get::<TempoExportPreview>("tempoExportPreview")
            .ok()
            .flatten()
    });
    let intent = Signal::derive(move || {
        entry
            .get()
            .get::<TempoExportIntent>("tempoExport")
            .ok()
            .flatten()
    });
    let key = Signal::derive(move || {
        serde_json::json!({"entry":entry.get(),"card":card.get()}).to_string()
    });
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let reset_open = RwSignal::new(false);
    let evidence = RwSignal::new(String::new());
    let risk = RwSignal::new(false);
    let uncertain = Memo::new(move |_| {
        entry
            .get()
            .string_equals("exportState", "uncertain")
            .unwrap_or(false)
    });
    let draft = Memo::new(move |_| {
        entry
            .get()
            .string_equals("exportState", "draft")
            .unwrap_or(false)
            && !entry.get().truthy("runningSince").unwrap_or(true)
    });
    let other_draft = Memo::new(move |_| {
        drafts.with(|drafts| drafts.iter().any(|other| other != &id.get_value()))
    });
    Effect::new(move |_| {
        let _ = key.get();
        let _ = evidence.get();
        approved.set(String::new());
        risk.set(false);
    });
    Effect::new(move |_| {
        drafts.update(|drafts| {
            if reset_open.get() || !evidence.get().is_empty() {
                drafts.insert(id.get_value());
            } else {
                drafts.remove(&id.get_value());
            }
        })
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = drafts.try_update(|drafts| {
            drafts.remove(&id.get_value());
        });
    });
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked()
            || (action != "reconcile" && (blocked.get_untracked() || other_draft.get_untracked()))
        {
            return;
        }
        if ["export-preview", "export"].contains(&action) && (archived || !draft.get_untracked()) {
            return;
        }
        if action == "export"
            && (preview.get_untracked().is_none()
                || approved.get_untracked() != key.get_untracked())
        {
            return;
        }
        if action == "reconcile" && !uncertain.get_untracked() {
            return;
        }
        if action == "reset-export"
            && (!uncertain.get_untracked()
                || !risk.get_untracked()
                || evidence.get_untracked().trim().is_empty())
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let Some(expected_card) = card.get_untracked() else {
            return;
        };
        let expected = entry.get_untracked();
        let manual_evidence = evidence.get_untracked();
        let path = format!("/api/time/{}/{action}", id.get_value());
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result=async {
                let body=if action=="reset-export" {serde_json::json!({"expected":expected,"confirmed":true,"acceptDuplicateRisk":true,"manualEvidence":manual_evidence})}
                else {
                    let connection:TempoConnection=read(&credential,"/api/tempo/config").await?.json().await.map_err(|_|"Réponse du serveur invalide.".to_owned())?;
                    serde_json::to_value(TempoExportRequest {expected,expected_card:atelier_core::application::jira::card_snapshot(&expected_card),jira_revision:connection.jira_revision,tempo_revision:connection.configuration.revision,confirmed:action!="export-preview"}).expect("Tempo export serialization")
                };
                write(&credential,Method::POST,&path,&body).await?.json::<Document>().await.map_err(|_|"Réponse du serveur invalide.".to_owned())
            }.await;
            if alive.get() {
                match result {
                    Ok(saved) => {
                        board.times.update(|entries| {
                            if let Some(current) = entries
                                .iter_mut()
                                .find(|entry| field(entry, "id") == id.get_value())
                            {
                                *current = saved;
                            }
                        });
                        if action == "reset-export" {
                            reset_open.set(false);
                            evidence.set(String::new());
                            risk.set(false);
                            notice.set("Saisie débloquée par confirmation manuelle. L’historique incertain est conservé ; aucun envoi effectué.".into());
                        } else {
                            notice.set(if action=="export-preview"{"Aperçu Tempo préparé. Aucun temps envoyé."}else{"Journal Tempo observé avec les données attendues. Aucun nouvel envoi automatique."}.into());
                        }
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    });
    view! {<div class="tempo-export" hidden=move||entry.get().raw("connectorExport").is_some()||(card.get().is_none_or(|card|card.fields.get::<Document>("ticket").ok().flatten().is_none_or(|ticket|!ticket.string_equals("connector","jira").unwrap_or(false)))&&intent.get().is_none())>
        <button class="preview-time-export" type="button" hidden=move||!draft.get() disabled=move||pending.get()||blocked.get()||other_draft.get()||archived on:click=move|_|execute.with_value(|execute|execute("export-preview"))>{move||i18n.text("Préparer l’export Tempo")}</button>
        {move||preview.get().filter(|_|draft.get()).map(|preview|view!{<div class="confirmation"><p>{move||i18n.text("Vérifie le site, le compte Jira, le ticket et les données avant l’envoi.")}</p><dl><dt>{move||i18n.text("Site Jira")}</dt><dd>{preview.jira_url}</dd><dt>"Tempo"</dt><dd>{preview.tempo.base}</dd><dt>{move||i18n.text("Compte Jira")}</dt><dd>{preview.payload.author_account_id}</dd><dt>{move||i18n.text("Ticket")}</dt><dd>{field(&preview.ticket,"key")}</dd><dt>{move||i18n.text("Date et heure locales")}</dt><dd>{format!("{} {}",preview.payload.start_date,preview.payload.start_time)}</dd><dt>{move||i18n.text("Durée (secondes)")}</dt><dd>{preview.payload.time_spent_seconds}</dd></dl><p>{field(&preview.entry,"description")}</p><p class="field-help">{move||i18n.text("Tempo interprète cette date et cette heure selon le profil Jira de l’auteur. Aucun coût ni temps agent n’est envoyé.")}</p><label><input class="confirm-time-export" type="checkbox" disabled=move||pending.get()||blocked.get()||other_draft.get()||archived prop:checked=move||approved.get()==key.get() on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked()}else{String::new()}) />{move||i18n.text("Confirmer l’envoi de ce temps humain à ce ticket.")}</label><button class="send-time-export" type="button" disabled=move||pending.get()||blocked.get()||other_draft.get()||archived||approved.get()!=key.get() on:click=move|_|execute.with_value(|execute|execute("export"))>{move||i18n.text("Envoyer le temps confirmé")}</button></div>})}
        <div class="confirmation" hidden=move||!uncertain.get()><p>{move||i18n.text("Envoi Tempo incertain. La relecture ne crée aucun journal ; une absence ne prouve pas qu’un envoi retardé ne sera jamais appliqué.")}</p><button class="reconcile-time-export" type="button" disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute("reconcile"))>{move||i18n.text("Rechercher le journal Tempo")}</button><button class="open-time-export-reset" type="button" disabled=move||pending.get()||blocked.get()||other_draft.get() on:click=move|_|reset_open.set(true)>{move||i18n.text("Débloquer après vérification manuelle…")}</button></div>
        <div class="confirmation" hidden=move||!reset_open.get()><p role="alert">{move||i18n.text("Un nouvel envoi peut créer un doublon si l’ancien arrive plus tard. Vérifie Tempo et consigne ta décision ; Atelier ne peut pas certifier l’absence distante.")}</p><label>{move||i18n.text("Vérification manuelle dans Tempo")}<textarea class="time-export-evidence" maxlength="1000" disabled=move||pending.get() prop:value=move||evidence.get() on:input=move|event|evidence.set(event_target_value(&event)) /></label><label><input class="time-export-risk" type="checkbox" disabled=move||pending.get() prop:checked=move||risk.get() on:change=move|event|risk.set(event_target_checked(&event)) />{move||i18n.text("J’ai vérifié Tempo et j’accepte explicitement le risque de doublon.")}</label><button class="reset-time-export" type="button" disabled=move||pending.get()||blocked.get()||other_draft.get()||!uncertain.get()||!risk.get()||evidence.get().trim().is_empty() on:click=move|_|execute.with_value(|execute|execute("reset-export"))>{move||i18n.text("Conserver l’historique et débloquer")}</button><button type="button" disabled=move||pending.get() on:click=move|_|{reset_open.set(false);evidence.set(String::new());risk.set(false);} >{move||i18n.text("Annuler")}</button></div>
        {move||intent.get().and_then(|intent|intent.worklog_id).map(|worklog|view!{<p>{move||i18n.text("Identifiant Tempo reçu :")}" "{worklog}</p>})}
        {move||intent.get().and_then(|intent|intent.observed_at).map(|observed|view!{<p>{move||i18n.text("Observation Tempo :")}" "<time datetime=observed.clone()>{observed}</time></p>})}
        <p class="time-export-error error" role="alert">{move||i18n.text(&error.get())}</p><p class="time-export-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </div>}
}
