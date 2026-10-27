use crate::{i18n::I18n, read};
use atelier_core::contracts::{
    Card, ForgeCheckScope as Scope, ForgeCheckState as State, ForgeChecks, ForgeConnections,
    ForgeReviewDecision as Decision, PullRequest,
};
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
fn state(value: State) -> &'static str {
    match value {
        State::Pending => "En attente",
        State::Running => "En cours",
        State::Succeeded => "Réussite déclarée",
        State::Failed => "Échec déclaré",
        State::Cancelled => "Annulé",
        State::Skipped => "Ignoré ou neutre",
        State::Unknown => "État indisponible",
    }
}
fn scope(value: Scope) -> &'static str {
    match value {
        Scope::SourceCommit => "Commit source",
        Scope::OtherSnapshot => "Autre commit ou itération",
        Scope::Unscoped => "Sans commit attribué",
    }
}
fn decision(value: Decision) -> &'static str {
    match value {
        Decision::Approved => "Approbation déclarée",
        Decision::ApprovedWithSuggestions => "Approbation avec suggestions",
        Decision::ChangesRequested => "Modifications demandées",
        Decision::Waiting => "En attente de l’auteur",
        Decision::Dismissed => "Avis retiré",
        Decision::None => "Aucun vote",
        Decision::Unknown => "Avis indisponible",
    }
}
#[component]
pub fn ForgeChecksView(
    card: Signal<Option<Card>>,
    configuration: Signal<Option<ForgeConnections>>,
    token: StoredValue<Result<String, String>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let result = RwSignal::new(None::<ForgeChecks>);
    let page = RwSignal::new(0usize);
    let reviewer_page = RwSignal::new(0usize);
    let linked = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<PullRequest>("pullRequest").ok().flatten())
    });
    let context = Signal::derive(move || {
        Some(
            serde_json::json!({"pr":linked.get()?,"revision":configuration.get()?.revision})
                .to_string(),
        )
    });
    Effect::new(move |previous: Option<Option<String>>| {
        let current = context.get();
        if previous.as_ref() != Some(&current) {
            result.set(None);
            page.set(0);
            reviewer_page.set(0);
            error.set(String::new());
        }
        current
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let inspect = move |_| {
        if loading.get_untracked() {
            return;
        }
        let Some(pr) = linked.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        if !configuration.connections.iter().any(|connection| {
            connection.configuration.kind == pr.kind && connection.configuration.base == pr.base_url
        }) {
            error.set("La connexion de cette PR/MR a changé. Recharge les connexions et relis la liaison.".into());
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let params = match web_sys::UrlSearchParams::new() {
            Ok(params) => params,
            Err(_) => {
                error.set("Paramètres de lecture invalides.".into());
                return;
            }
        };
        params.set("kind", pr.kind.name());
        params.set("revision", &configuration.revision.to_string());
        params.set("repository", &pr.repository);
        params.set("number", &pr.number.to_string());
        let path = format!("/api/forges/checks?{}", String::from(params.to_string()));
        let expected = context.get_untracked();
        let revision = configuration.revision;
        loading.set(true);
        result.set(None);
        error.set(String::new());
        page.set(0);
        reviewer_page.set(0);
        let alive = alive.clone();
        spawn_local(async move {
            let fetched = async {
                let report = read(&credential, &path)
                    .await?
                    .json::<ForgeChecks>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                if report.pull_request.revision != revision
                    || !atelier_core::application::forge::same_pull_request(
                        &pr,
                        &report.pull_request,
                    )
                {
                    return Err(
                        "La PR/MR a évolué. Actualise sa liaison avant de relire les contrôles."
                            .into(),
                    );
                }
                Ok(report)
            }
            .await;
            if alive.get() {
                loading.set(false);
                if context.get_untracked() == expected {
                    match fetched {
                        Ok(report) => result.set(Some(report)),
                        Err(message) => error.set(message),
                    }
                } else {
                    error.set("La liaison ou les identifiants ont changé pendant la lecture. Relis les contrôles.".into());
                }
            }
        });
    };
    let count = Memo::new(move |_| {
        result.with(|result| result.as_ref().map_or(0, |report| report.checks.len()))
    });
    let visible = Memo::new(move |_| {
        result.with(|result| {
            result
                .as_ref()
                .map(|report| {
                    report
                        .checks
                        .iter()
                        .skip(page.get() * 100)
                        .take(100)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    let reviewer_count = Memo::new(move |_| {
        result.with(|result| result.as_ref().map_or(0, |report| report.reviewers.len()))
    });
    view! {<section class="task-files" hidden=move||linked.with(|linked|linked.is_none()) aria-labelledby="forge-checks-heading" aria-busy=move||loading.get().to_string()><h4 id="forge-checks-heading">{move||i18n.text("Contrôles et avis de la forge")}</h4>
        <p class="field-help">{move||i18n.text("États déclarés par la forge, sans lancement de CI. Cette lecture ne valide ni les critères de la tâche, ni les règles de fusion. Une liste vide ne signifie pas réussite.")}</p>
        <button id="read-forge-checks" type="button" disabled=move||loading.get()||configuration.with(|configuration|configuration.is_none()) on:click=inspect>{move||i18n.text("Lire les contrôles et avis")}</button>
        <p id="forge-checks-error" class="error" role="alert">{move||i18n.text(&error.get())}</p>
        <p role="status">{move||if loading.get(){i18n.text("Lecture en cours…")}else{result.with(|result|result.as_ref().map(|report|format!("{} · {}",report.pull_request.source_commit.as_deref().unwrap_or(""),report.pull_request.checked_at)).unwrap_or_default())}}</p>
        <Show when=move||result.with(|result|result.is_some())>
            <p hidden=move||count.get()!=0>{move||i18n.text("Aucun contrôle observé pour ce snapshot. Aucune réussite déduite.")}</p>
            <div class="table-scroll" tabindex="0" role="region" aria-label=move||i18n.text("Résultats des contrôles")><table class="report-table"><thead><tr><th>{move||i18n.text("Contrôle")}</th><th>{move||i18n.text("État déclaré")}</th><th>{move||i18n.text("Portée")}</th><th>{move||i18n.text("Détail")}</th></tr></thead><tbody>{move||visible.get().into_iter().map(|check|view!{<tr><td>{check.name}</td><td>{i18n.text(state(check.state))}</td><td>{i18n.text(scope(check.scope))}</td><td>{check.description}</td></tr>}).collect_view()}</tbody></table></div>
            <button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled={move||(page.get()+1)*100>=count.get()} on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button>
            <p>{move||result.with(|result|result.as_ref().map(|report|i18n.text("Contrôles d’autres snapshots écartés : {0}").replace("{0}",&report.other_snapshot_count.to_string())).unwrap_or_default())}</p>
            <h5>{move||i18n.text("Avis humains déclarés")}</h5><p hidden=move||result.with(|result|result.as_ref().is_some_and(|report|report.reviewers_available))>{move||i18n.text("Avis indisponibles. Aucune absence d’avis déduite.")}</p>
            <p hidden=move||reviewer_count.get()!=0||result.with(|result|result.as_ref().is_none_or(|report|!report.reviewers_available))>{move||i18n.text("Aucun avis humain observé.")}</p>
            <ul>{move||result.with(|result|result.as_ref().map(|report|report.reviewers.iter().skip(reviewer_page.get()*100).take(100).cloned().collect::<Vec<_>>()).unwrap_or_default()).into_iter().map(|reviewer|view!{<li>{reviewer.name}" · "{i18n.text(decision(reviewer.decision))}" · "{i18n.text(scope(reviewer.scope))}" · "<code>{reviewer.commit.unwrap_or_else(||i18n.text("Commit non fourni"))}</code></li>}).collect_view()}</ul>
            <div role="group" aria-label=move||i18n.text("Pages des avis humains")><button type="button" disabled=move||reviewer_page.get()==0 on:click=move|_|reviewer_page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled={move||(reviewer_page.get()+1)*100>=reviewer_count.get()} on:click=move|_|reviewer_page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button></div>
            <ul>{move||result.with(|result|result.as_ref().map(|report|report.limits.clone()).unwrap_or_default()).into_iter().map(|limit|view!{<li>{i18n.text(&limit)}</li>}).collect_view()}</ul>
        </Show>
    </section>}
}
