use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::{
    application::reviews::{Context, Coverage, Review},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone)]
struct ContextView {
    source: String,
    target: String,
    base: String,
    requirements: serde_json::Value,
}

#[component]
pub fn ReviewResult(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    run: Signal<Option<Document>>,
    agent: RwSignal<String>,
    reply_drafts: RwSignal<BTreeSet<String>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card_id = StoredValue::new(card_id);
    let error = RwSignal::new(String::new());
    let selections = RwSignal::new(BTreeMap::<String, BTreeSet<usize>>::new());
    let draft_id = StoredValue::new(format!("review-selection:{}", card_id.get_value()));
    Effect::new(move |_| {
        reply_drafts.update(|drafts| {
            if selections.with(|selections| selections.values().any(|indexes| !indexes.is_empty()))
            {
                drafts.insert(draft_id.get_value());
            } else {
                drafts.remove(&draft_id.get_value());
            }
        })
    });
    on_cleanup(move || {
        let _ = reply_drafts.try_update(|drafts| {
            drafts.remove(&draft_id.get_value());
        });
    });
    let selection = Signal::derive(move || {
        run.get()
            .map(|run| {
                selections.with(|selections| {
                    selections
                        .get(&field(&run, "id"))
                        .cloned()
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default()
    });
    let report = Signal::derive(move || {
        run.get()
            .and_then(|run| run.get::<Review>("review").ok().flatten())
    });
    let context = Signal::derive(move || {
        run.get().and_then(|run| {
            if let Ok(Some(context)) = run.get::<Context>("reviewContext") {
                Some(ContextView {
                    source: context.source,
                    target: context.target,
                    base: context.base,
                    requirements: context.requirements,
                })
            } else {
                run.get::<atelier_core::application::extensions::RepositoryReviewContext>(
                    "connectorReviewContext",
                )
                .ok()
                .flatten()
                .map(|context| ContextView {
                    source: context.source,
                    target: context.target,
                    base: context.base,
                    requirements: context.requirements,
                })
            }
        })
    });
    let validate = move |_| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(run) = run.get_untracked() else {
            return;
        };
        let Some(expected) = board.cards.with_untracked(|cards| {
            cards
                .iter()
                .find(|card| card.id == card_id.get_value())
                .cloned()
        }) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/cards/{}/runs/{}/review/validate",
            card_id.get_value(),
            field(&run, "id")
        );
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &path,
                &serde_json::json!({"expected":expected,"confirmed":true}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    };
    let correct = move |_| {
        if pending.get_untracked() || blocked.get_untracked() || agent.get_untracked().is_empty() {
            return;
        }
        let Some(selected) = run.get_untracked() else {
            return;
        };
        let selected_id = field(&selected, "id");
        let Some(expected) = board.cards.with_untracked(|cards| {
            cards
                .iter()
                .find(|card| card.id == card_id.get_value())
                .cloned()
        }) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let indexes = selection.get_untracked().into_iter().collect::<Vec<_>>();
        if indexes.is_empty() {
            return;
        }
        let body = serde_json::json!({"expected":expected,"agent":agent.get_untracked(),"mode":"edit","purpose":"correction","reviewId":selected_id,"indexes":indexes,"confirmed":true});
        let path = format!("/api/cards/{}/runs/start", card_id.get_value());
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(&credential, Method::POST, &path, &body).await;
            if result.is_ok() {
                let _ = selections.try_update(|selections| {
                    selections.remove(&selected_id);
                });
            }
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    };
    view! {<section class="task-files" id="review-result" hidden=move||run.get().is_none_or(|run|field(&run,"kind")!="pr-review") aria-labelledby="review-heading">
        <h4 id="review-heading">{move||i18n.text("Review de la PR/MR")}</h4>
        <p class="field-help">{move||i18n.text("La review décrit les commits examinés et les preuves disponibles. Elle ne coche aucun critère et ne publie aucun commentaire.")}</p>
        <p class="error" role="alert">{move||run.get().map(|run|i18n.text(&field(&run,"reviewError"))).unwrap_or_default()}</p>
        <p role="status" id="review-freshness">{move||run.get().map(|run|{let stale=field(&run,"reviewStaleReason");if !stale.is_empty(){i18n.text(&stale)}else{format!("{} {}",i18n.text("Revalidée à :"),field(&run,"reviewValidatedAt"))}}).unwrap_or_default()}</p>
        <dl hidden=move||context.get().is_none()><dt>{move||i18n.text("Commit source examiné")}</dt><dd><code>{move||context.get().map(|context|context.source).unwrap_or_default()}</code></dd><dt>{move||i18n.text("Commit cible examiné")}</dt><dd><code>{move||context.get().map(|context|context.target).unwrap_or_default()}</code></dd><dt>{move||i18n.text("Base du diff examiné")}</dt><dd><code>{move||context.get().map(|context|context.base).unwrap_or_default()}</code></dd></dl>
        <p id="review-summary">{move||report.get().map(|review|review.summary).unwrap_or_default()}</p>
        <p hidden=move||report.get().is_none_or(|review|!review.findings.is_empty())>{move||i18n.text("Aucun défaut étayé dans cette review ; consulte ses limites.")}</p>
        <For each=move||report.get().map(|review|review.findings.into_iter().enumerate().collect::<Vec<_>>()).unwrap_or_default() key=move|(index,_)|format!("{}:{index}",run.get().map(|run|field(&run,"id")).unwrap_or_default()) children=move|(index,finding)|view!{<div><label class="check-filter"><input class="review-selection" type="checkbox" prop:checked=move||selection.with(|selection|selection.contains(&index)) disabled=move||pending.get() on:change=move|event|{let selected=event_target_checked(&event);if let Some(run)=run.get_untracked(){selections.update(|selections|{let indexes=selections.entry(field(&run,"id")).or_default();if selected{indexes.insert(index);}else{indexes.remove(&index);}});}} />{move||i18n.text("Sélectionner ce constat pour correction")}</label><details class="review-finding"><summary>{format!("{} · {}",finding.priority.label(),finding.title)}</summary><p><code>{finding.file}{finding.line.map(|line|format!(":{line}")).unwrap_or_default()}</code></p><p>{finding.problem}</p><p>{finding.suggestion}</p></details></div>} />
        <h5>{move||i18n.text("Bilan des critères examinés")}</h5>
        <p hidden=move||report.get().is_none_or(|review|!review.criteria.is_empty())>{move||i18n.text("Aucun critère fourni à la review.")}</p>
        <For each=move||report.get().map(|review|review.criteria).unwrap_or_default() key=move|criterion|format!("{}:{}",run.get().map(|run|field(&run,"id")).unwrap_or_default(),criterion.index) children=move|criterion|{
            let text=context.get_untracked().and_then(|context|context.requirements["criteria"][criterion.index].as_str().map(str::to_owned)).unwrap_or_default();
            let status=match criterion.status{Coverage::Covered=>"Couvert selon la review",Coverage::NotCovered=>"Non couvert selon la review",Coverage::Unverifiable=>"Non vérifiable"};
            view!{<details class="review-criterion"><summary>{text}{" · "}{move||i18n.text(status)}</summary><p>{criterion.evidence}</p></details>}
        } />
        <button id="validate-review" type="button" hidden=move||report.get().is_none() disabled=move||pending.get()||blocked.get() on:click=validate>{move||i18n.text("Revalider le besoin et les commits actuels")}</button>
        <button id="correct-review" type="button" hidden=move||report.get().is_none() disabled=move||pending.get()||blocked.get()||selection.with(BTreeSet::is_empty)||agent.get().is_empty()||run.get().is_none_or(|run|!field(&run,"reviewStaleReason").is_empty()) on:click=correct>{move||i18n.text("Confirmer et corriger les constats sélectionnés")}</button>
        <p class="field-help">{move||i18n.text("La correction utilise le profil choisi dans Exécutions, en mode Modification. Les commits et le besoin sont relus avant lancement ; une nouvelle review sera nécessaire après la correction.")}</p>
        <p class="field-help">{move||i18n.text("La revalidation relit la PR/MR et télécharge sa cible sans changer les branches. Toute évolution des commits ou du besoin exige une nouvelle review. Cette date décrit un instantané, pas une surveillance distante continue.")}</p>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
