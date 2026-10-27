use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::contracts::{
    Card, JiraConfiguration, JiraRefreshApply, JiraRefreshPreview, JiraRefreshRequest,
    JiraRefreshState,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, collections::BTreeSet, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[derive(Clone)]
struct Row {
    card: Card,
    selected: bool,
    status: bool,
    attempted: bool,
    result: String,
}

#[component]
pub fn JiraBatch(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let selected = RwSignal::new(BTreeSet::<String>::new());
    let rows = RwSignal::new(Vec::<Row>::new());
    let confirmed = RwSignal::new(false);
    let cancelled = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    Effect::new(move |_| {
        dirty.set(
            !selected.with(BTreeSet::is_empty)
                || rows.with(|rows| rows.iter().any(|row| row.selected && !row.attempted)),
        )
    });
    let context = Memo::new(move |_| {
        (
            serde_json::to_string(&board.cards.get()).unwrap_or_default(),
            serde_json::to_string(&board.settings.get()).unwrap_or_default(),
            serde_json::to_string(&board.projects.get()).unwrap_or_default(),
        )
    });
    Effect::new(move |_| {
        let _ = context.get();
        confirmed.set(false);
    });
    let candidates = Signal::derive(move || {
        board
            .cards
            .get()
            .into_iter()
            .filter(|card| {
                !card.fields.truthy("archived").unwrap_or(true)
                    && card
                        .fields
                        .get::<atelier_core::document::Document>("ticket")
                        .ok()
                        .flatten()
                        .is_some_and(|ticket| field(&ticket, "connector") == "jira")
            })
            .collect::<Vec<_>>()
    });
    let reading_alive = alive.clone();
    let recover = move |_| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let ids = selected.get_untracked();
        rows.set(
            candidates
                .get_untracked()
                .into_iter()
                .filter(|card| ids.contains(&card.id))
                .map(|card| {
                    let ready = card
                        .fields
                        .get::<JiraRefreshPreview>("jiraRefresh")
                        .ok()
                        .flatten()
                        .is_some_and(|preview| preview.state == JiraRefreshState::Ready);
                    Row {
                        card,
                        selected: false,
                        status: false,
                        attempted: !ready,
                        result: if ready {
                            String::new()
                        } else {
                            "Aperçu Jira absent.".into()
                        },
                    }
                })
                .collect(),
        );
        confirmed.set(false);
    };
    let prepare = move |_| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let ids = selected.get_untracked();
        if ids.is_empty() || ids.len() > 20 {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let cards = candidates
            .get_untracked()
            .into_iter()
            .filter(|card| ids.contains(&card.id))
            .collect::<Vec<_>>();
        if cards.len() != ids.len() {
            error.set("La sélection Jira a changé. Vérifie les cartes.".into());
            return;
        }
        let revision = board
            .settings
            .get_untracked()
            .get::<JiraConfiguration>("jiraConnection")
            .ok()
            .flatten()
            .map(|configuration| configuration.revision)
            .unwrap_or(0);
        pending.set(true);
        cancelled.set(false);
        confirmed.set(false);
        rows.set(Vec::new());
        error.set(String::new());
        let alive = reading_alive.clone();
        spawn_local(async move {
            for card in cards {
                if !alive.get() || cancelled.get_untracked() {
                    break;
                }
                let result = async {
                    let body = serde_json::to_value(JiraRefreshRequest {
                        revision,
                        expected: atelier_core::application::jira::card_snapshot(&card),
                    })
                    .map_err(|_| "Requête invalide.".to_owned())?;
                    write(
                        &credential,
                        Method::POST,
                        &format!("/api/cards/{}/jira/refresh-preview", card.id),
                        &body,
                    )
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                if !alive.get() {
                    break;
                }
                rows.update(|rows| match result {
                    Ok(card) => rows.push(Row {
                        card,
                        selected: false,
                        status: false,
                        attempted: false,
                        result: String::new(),
                    }),
                    Err(message) => rows.push(Row {
                        card,
                        selected: false,
                        status: false,
                        attempted: true,
                        result: message,
                    }),
                });
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    let applying_alive = alive.clone();
    let apply = move |_| {
        if pending.get_untracked() || blocked.get_untracked() || !confirmed.get_untracked() {
            return;
        }
        let requests = rows
            .get_untracked()
            .into_iter()
            .filter(|row| row.selected && !row.attempted)
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        pending.set(true);
        cancelled.set(false);
        confirmed.set(false);
        error.set(String::new());
        let alive = applying_alive.clone();
        spawn_local(async move {
            for row in requests {
                if !alive.get() || cancelled.get_untracked() {
                    break;
                }
                let result = async {
                    let preview = row
                        .card
                        .fields
                        .get::<JiraRefreshPreview>("jiraRefresh")
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?
                        .filter(|preview| preview.state == JiraRefreshState::Ready)
                        .ok_or_else(|| "Aperçu Jira absent.".to_owned())?;
                    let body = serde_json::to_value(JiraRefreshApply {
                        revision: preview.revision,
                        expected: atelier_core::application::jira::card_snapshot(&row.card),
                        preview_id: preview.id,
                        apply_status: row.status,
                    })
                    .map_err(|_| "Requête invalide.".to_owned())?;
                    write(
                        &credential,
                        Method::POST,
                        &format!("/api/cards/{}/jira/refresh-apply", row.card.id),
                        &body,
                    )
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                if !alive.get() {
                    break;
                }
                rows.update(|rows| {
                    if let Some(saved) = rows.iter_mut().find(|saved| saved.card.id == row.card.id)
                    {
                        saved.attempted = true;
                        saved.selected = false;
                        saved.result = match result {
                            Ok(_) => "Aperçu appliqué.".into(),
                            Err(message) => message,
                        };
                    }
                });
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    view! {<section class="task-files" aria-labelledby="jira-batch-heading">
        <h3 id="jira-batch-heading">{move||i18n.text("Synchroniser plusieurs cartes Jira")}</h3>
        <p class="field-help">{move||i18n.text("Sélectionne au plus vingt cartes, relis leurs aperçus puis choisis lesquels appliquer. Chaque carte est vérifiée et enregistrée séparément ; les erreurs restent visibles et ne sont pas rejouées.")}</p>
        <fieldset disabled=move||pending.get()||blocked.get()><For each=move||candidates.get() key=|card|card.id.clone() children=move|card|{let id=StoredValue::new(card.id);view!{<label><input class="jira-batch-select" data-jira-card=id.get_value() type="checkbox" prop:checked=move||selected.with(|selected|selected.contains(&id.get_value())) disabled=move||selected.with(|selected|selected.len()>=20&&!selected.contains(&id.get_value())) on:change=move|event|{confirmed.set(false);selected.update(|selected|{if event_target_checked(&event){selected.insert(id.get_value());}else{selected.remove(&id.get_value());}});}/>{String::from_utf16_lossy(&card.title.0)}</label>}}/>
        <button id="prepare-jira-batch" type="button" disabled=move||selected.with(BTreeSet::is_empty) on:click=prepare>{move||i18n.text("Relire la sélection Jira")}</button>
        <button id="recover-jira-batch" type="button" disabled=move||selected.with(BTreeSet::is_empty) on:click=recover>{move||i18n.text("Reprendre les aperçus existants")}</button></fieldset>
        <For each=move||rows.get() key=|row|(row.card.id.clone(),row.attempted) children=move|row|{
            let id=StoredValue::new(row.card.id.clone());
            let preview=row.card.fields.get::<JiraRefreshPreview>("jiraRefresh").ok().flatten();
            let ready=preview.as_ref().is_some_and(|preview|preview.state==JiraRefreshState::Ready)&&!row.attempted;
            let mapped=preview.as_ref().and_then(|preview|preview.mapped_status.clone());
            let old_title=String::from_utf16_lossy(&row.card.title.0);
            view!{<article class="task-files" data-jira-card=id.get_value()><h4>{old_title}</h4><p>{field(&row.card.fields,"description")}</p>{preview.and_then(|preview|preview.detail).map(|detail|view!{<p>{detail.issue.title}</p><pre>{detail.description}</pre>})}
                <label><input class="jira-batch-apply" type="checkbox" disabled=move||!ready||pending.get()||blocked.get() prop:checked=move||rows.with(|rows|rows.iter().any(|row|row.card.id==id.get_value()&&row.selected)) on:change=move|event|{confirmed.set(false);rows.update(|rows|{if let Some(row)=rows.iter_mut().find(|row|row.card.id==id.get_value()){row.selected=event_target_checked(&event);}});}/>{move||i18n.text("Appliquer cet aperçu à la carte")}</label>
                {mapped.map(|status|{let label=i18n.text(crate::STATUSES.iter().find(|(key,_)|*key==status).map(|(_,label)|*label).unwrap_or("En attente"));view!{<label><input class="jira-batch-status" type="checkbox" disabled=move||!ready||pending.get()||blocked.get() prop:checked=move||rows.with(|rows|rows.iter().any(|row|row.card.id==id.get_value()&&row.status)) on:change=move|event|{confirmed.set(false);rows.update(|rows|{if let Some(row)=rows.iter_mut().find(|row|row.card.id==id.get_value()){row.status=event_target_checked(&event);}});}/>{move||i18n.text("Appliquer aussi la colonne proposée : ")}{label}</label>}})}
                <p role="status">{i18n.text(&row.result)}</p></article>}
        }/>
        <label><input id="confirm-jira-batch" type="checkbox" disabled=move||pending.get()||blocked.get()||!rows.with(|rows|rows.iter().any(|row|row.selected&&!row.attempted)) prop:checked=move||confirmed.get() on:change=move|event|confirmed.set(event_target_checked(&event))/>{move||i18n.text("Appliquer les aperçus cochés, avec les changements de colonne sélectionnés.")}</label>
        <button id="apply-jira-batch" type="button" disabled=move||pending.get()||blocked.get()||!confirmed.get() on:click=apply>{move||i18n.text("Appliquer la sélection Jira")}</button>
        <button type="button" hidden=move||!pending.get() on:click=move |_|cancelled.set(true)>{move||i18n.text("Arrêter après la carte en cours")}</button>
        <button id="clear-jira-batch" type="button" disabled=move||pending.get() on:click=move |_|{rows.set(Vec::new());selected.set(BTreeSet::new());confirmed.set(false);}>{move||i18n.text("Effacer la sélection")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
