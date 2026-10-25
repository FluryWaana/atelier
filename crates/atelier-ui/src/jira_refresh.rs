use crate::{field, i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, JiraConfiguration, JiraRefreshApply, JiraRefreshPreview, JiraRefreshRequest,
    JiraRefreshState as Phase,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn JiraRefresh(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    on_applied: Callback<Card>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<JiraRefreshPreview>("jiraRefresh")
                .ok()
                .flatten()
        })
    });
    let linked = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref().is_some_and(|card| {
                card.fields
                    .get::<atelier_core::document::Document>("ticket")
                    .ok()
                    .flatten()
                    .is_some_and(|ticket| field(&ticket, "connector") == "jira")
            })
        })
    });
    let busy = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref().is_some_and(|card| {
                atelier_core::application::jira::unresolved(&card.fields).unwrap_or(true)
            })
        })
    });
    let revision = move || {
        board.settings.with(|settings| {
            settings
                .get::<JiraConfiguration>("jiraConnection")
                .ok()
                .flatten()
                .map(|config| config.revision)
                .unwrap_or(0)
        })
    };
    let confirmed = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let loading_alive = alive.clone();
    let load = move |_| {
        if pending.get_untracked() || blocked.get_untracked() || busy.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(JiraRefreshRequest {
            revision: revision(),
            expected: atelier_core::application::jira::card_snapshot(&expected),
        })
        .expect("Jira refresh serialization");
        pending.set(true);
        confirmed.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = loading_alive.clone();
        let path = format!("/api/cards/{}/jira/refresh-preview", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => replace_card(board.cards, card),
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    let applying_alive = alive.clone();
    let apply = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || busy.get_untracked()
            || confirmed.get_untracked().is_empty()
        {
            return;
        }
        let (Some(expected), Some(preview)) = (card.get_untracked(), preview.get_untracked())
        else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if confirmed.get_untracked() != preview.id {
            return;
        }
        let body = serde_json::to_value(JiraRefreshApply {
            revision: revision(),
            expected: atelier_core::application::jira::card_snapshot(&expected),
            preview_id: preview.id,
        })
        .expect("Jira refresh apply serialization");
        pending.set(true);
        confirmed.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = applying_alive.clone();
        let path = format!("/api/cards/{}/jira/refresh-apply", id.get_value());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => {
                        on_applied.run(card.clone());
                        replace_card(board.cards, card);
                        notice.set("Carte synchronisée depuis l’aperçu Jira. Aucun ticket distant modifié.".into());
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
    view! {<section class="task-files" hidden=move ||!linked.get() aria-labelledby="jira-refresh-heading" aria-busy=move ||pending.get().to_string()>
        <h3 id="jira-refresh-heading">{move ||i18n.text("Synchroniser le contenu Jira")}</h3><p class="field-help">{move ||i18n.text("Relis le ticket, compare l’aperçu, puis applique-le explicitement. Les critères et instructions locaux sont conservés.")}</p>
        <button id="load-jira-refresh" type="button" disabled=move ||pending.get()||blocked.get()||busy.get() on:click=load>{move ||i18n.text("Relire le ticket pour comparer")}</button>
        <p role="status" hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Reading))>{move ||i18n.text("Lecture du ticket pour préparer l’aperçu…")}</p>
        <p role="status" hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Interrupted))>{move ||i18n.text("La lecture précédente a été interrompue. Tu peux relire le ticket.")}</p>
        {move ||preview.get().filter(|preview|preview.state==Phase::Ready).and_then(|preview|preview.detail.map(|detail|view!{<div><h4>{move ||i18n.text("Contenu actuel de la carte")}</h4><p>{String::from_utf16_lossy(&preview.expected.title.0)}</p><pre>{field(&preview.expected.fields,"description")}</pre><h4>{move ||i18n.text("Contenu relu depuis Jira")}</h4><p>{detail.issue.title}</p><pre>{detail.description}</pre></div>}))}
        <div hidden=move ||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.state!=Phase::Ready))><p class="field-help">{move ||i18n.text("L’aperçu expire après dix minutes. Une modification concurrente impose une nouvelle lecture.")}</p><label><input id="confirm-jira-refresh" type="checkbox" disabled=move ||pending.get()||blocked.get()||busy.get() prop:checked=move ||preview.with(|preview|preview.as_ref().is_some_and(|preview|preview.id==confirmed.get())) on:change=move |event|confirmed.set(if event_target_checked(&event){preview.with(|preview|preview.as_ref().map(|preview|preview.id.clone()).unwrap_or_default())}else{String::new()}) />{move ||i18n.text("Remplacer le titre et la description locaux par cet aperçu Jira.")}</label><button id="apply-jira-refresh" type="button" disabled=move ||pending.get()||blocked.get()||busy.get()||preview.with(|preview|preview.as_ref().is_none_or(|preview|preview.id!=confirmed.get())) on:click=apply>{move ||i18n.text("Appliquer cet aperçu à la carte")}</button></div>
        <p class="error" role="alert">{move ||i18n.text(&error.get())}</p><p role="status">{move ||i18n.text(&notice.get())}</p>
    </section>}
}
