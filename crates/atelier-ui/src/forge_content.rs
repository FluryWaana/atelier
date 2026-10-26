use crate::{i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeContent, ForgeDraftRequest, PullRequest,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ForgeContentEditor(
    card: Signal<Option<Card>>,
    configuration: Signal<Option<ForgeConnections>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(None::<ForgeContent>);
    let draft = RwSignal::new(None::<ForgeContent>);
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let pr = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<PullRequest>("pullRequest").ok().flatten())
    });
    Effect::new(move |_| {
        let next = pr.get().map(|pr| ForgeContent {
            title: pr.title,
            body: pr.body,
        });
        if draft.get_untracked() == original.get_untracked() || next == draft.get_untracked() {
            original.set(next.clone());
            draft.set(next);
        }
    });
    Effect::new(move |_| dirty.set(draft.get() != original.get()));
    let key = Memo::new(move |_| {
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card.get()?),"revision":configuration.get()?.revision,"content":draft.get()?}).to_string())
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
    });
    let available = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            !card.fields.truthy("archived").unwrap_or(true)
                && !atelier_core::application::forge::unresolved(&card.fields).unwrap_or(true)
        }) && pr
            .get()
            .is_some_and(|pr| pr.status == "active" && pr.draft && !pr.fork)
    });
    let save = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || !available.get_untracked()
            || !dirty.get_untracked()
            || key.get_untracked().as_deref() != Some(approved.get_untracked().as_str())
        {
            return;
        }
        let (Some(expected), Some(configuration), Some(content)) = (
            card.get_untracked(),
            configuration.get_untracked(),
            draft.get_untracked(),
        ) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!("/api/cards/{}/forge/draft", expected.id);
        let body = serde_json::to_value(ForgeDraftRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            draft: true,
            confirmed: true,
            content: Some(content),
        })
        .expect("forge content request");
        pending.set(true);
        approved.set(String::new());
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
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
                        replace_card(board.cards, card);
                        notice.set("Contenu distant relu après envoi.".into());
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
    view! {<section class="task-files" hidden=move||pr.get().is_none() aria-labelledby="forge-content-heading">
        <h4 id="forge-content-heading">{move||i18n.text("Contenu de la PR/MR en brouillon")}</h4>
        <p class="field-help">{move||i18n.text("Compare le contenu relu, puis confirme les champs modifiés. Une édition distante intervenant entre la comparaison et l’envoi peut être remplacée par la forge ; évite les éditions simultanées de ces champs.")}</p>
        <details><summary>{move||i18n.text("Contenu distant relu")}</summary><p>{move||original.get().map(|content|content.title).unwrap_or_default()}</p><pre>{move||original.get().map(|content|content.body).unwrap_or_default()}</pre></details>
        <fieldset disabled=move||pending.get()||blocked.get()||!available.get()>
            <label for="forge-content-title">{move||i18n.text("Titre de la PR/MR")}</label><input id="forge-content-title" maxlength="255" prop:value=move||draft.get().map(|content|content.title).unwrap_or_default() on:input=move|event|draft.update(|draft|{if let Some(content)=draft{content.title=event_target_value(&event);}})/>
            <label for="forge-content-body">{move||i18n.text("Description de la PR/MR")}</label><textarea id="forge-content-body" maxlength=move||if pr.get().is_some_and(|pr|pr.kind==atelier_core::contracts::ForgeKind::Azure){"4000"}else{"20000"} rows="5" prop:value=move||draft.get().map(|content|content.body).unwrap_or_default() on:input=move|event|draft.update(|draft|{if let Some(content)=draft{content.body=event_target_value(&event);}})/>
            <label><input id="confirm-forge-content" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()})/>{move||i18n.text("Envoyer les modifications de contenu à cette PR/MR en brouillon.")}</label>
            <button id="save-forge-content" type="button" disabled=move||!dirty.get()||key.get().is_none_or(|key|key!=approved.get()) on:click=save>{move||i18n.text("Mettre à jour le contenu distant")}</button>
        </fieldset>
        <button id="reset-forge-content" type="button" disabled=move||pending.get()||!dirty.get() on:click=move |_|{draft.set(original.get_untracked());approved.set(String::new());}>{move||i18n.text("Annuler les modifications de contenu")}</button>
        <p id="forge-content-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
