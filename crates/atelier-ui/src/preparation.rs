use crate::{field, i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::{
    application::preparation::{self, Apply, Preparation},
    contracts::Card,
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn PreparationResult(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    run: Signal<Option<Document>>,
    on_applied: Callback<Card>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let result = Signal::derive(move || {
        run.get()
            .and_then(|run| run.get::<Preparation>("preparation").ok().flatten())
    });
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let current = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let stale = Memo::new(move |_| {
        let Some(card) = current.get() else {
            return true;
        };
        run.with(|run| {
            run.as_ref().is_none_or(|run| {
                run.get::<serde_json::Value>("requirements").ok().flatten()
                    != preparation::requirements(&card).ok()
            })
        })
    });
    let applied = Memo::new(move |_| {
        run.with(|run| {
            run.as_ref().is_some_and(|run| {
                run.get::<bool>("preparationApplied")
                    .ok()
                    .flatten()
                    .unwrap_or(false)
            })
        })
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let apply = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || stale.get_untracked()
            || applied.get_untracked()
        {
            return;
        }
        let Some(expected) = current.get_untracked() else {
            return;
        };
        let Some(run) = run.get_untracked() else {
            return;
        };
        let run_id = field(&run, "id");
        if approved.get_untracked() != run_id {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(Apply {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            run_id,
            confirmed: true,
        })
        .expect("preparation apply serialization");
        let path = format!("/api/cards/{}/preparation/apply", id.get_value());
        pending.set(true);
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
                        replace_card(board.cards, card.clone());
                        on_applied.run(card);
                        approved.set(String::new());
                        notice.set(
                            "Préparation appliquée. Les critères restent à valider manuellement."
                                .into(),
                        );
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
    view! {<section class="task-files" hidden=move||run.with(|run|run.as_ref().is_none_or(|run|field(run,"purpose")!="preparation")) aria-labelledby="preparation-heading"><h4 id="preparation-heading">{move||i18n.text("Préparation du ticket")}</h4><p class="error" role="alert">{move||run.with(|run|run.as_ref().map(|run|i18n.text(&field(run,"preparationError"))).unwrap_or_default())}</p>
        {move||result.get().map(|result|view!{<div><ul>{result.criteria.into_iter().map(|criterion|view!{<li>{criterion}</li>}).collect_view()}</ul><h5>{move||i18n.text("Questions à clarifier")}</h5><ul>{result.questions.into_iter().map(|question|view!{<li>{question}</li>}).collect_view()}</ul></div>})}
        <p role="status" hidden=move||!stale.get()||applied.get()>{move||i18n.text("Le besoin a changé depuis cette préparation. Relance l’analyse.")}</p><p role="status" hidden=move||!applied.get()>{move||i18n.text("Cette préparation a déjà été appliquée. Les validations humaines sont conservées.")}</p>
        <fieldset disabled=move||pending.get()||blocked.get()||stale.get()||applied.get() hidden=move||result.with(|result|result.is_none())><label><input id="confirm-preparation" type="checkbox" prop:checked=move||run.with(|run|run.as_ref().is_some_and(|run|field(run,"id")==approved.get())) on:change=move|event|approved.set(if event_target_checked(&event){run.with(|run|run.as_ref().map(|run|field(run,"id")).unwrap_or_default())}else{String::new()}) />{move||i18n.text("Remplacer les critères actuels par cette proposition et remettre leur validation humaine à zéro.")}</label><button id="apply-preparation" type="button" disabled=move||run.with(|run|run.as_ref().is_none_or(|run|field(run,"id")!=approved.get())) on:click=apply>{move||i18n.text("Confirmer l’application de la préparation")}</button></fieldset><p class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="preparation-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
