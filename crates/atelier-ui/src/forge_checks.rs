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
        State::Pending => "message.on_hold",
        State::Running => "message.in_progress",
        State::Succeeded => "message.reported_success",
        State::Failed => "message.reported_failure",
        State::Cancelled => "message.cancelled",
        State::Skipped => "message.skipped_or_neutral",
        State::Unknown => "message.state_unavailable",
    }
}
fn scope(value: Scope) -> &'static str {
    match value {
        Scope::SourceCommit => "message.source_commit",
        Scope::OtherSnapshot => "message.other_commit_or_iteration",
        Scope::Unscoped => "message.no_attributed_commit",
    }
}
fn decision(value: Decision) -> &'static str {
    match value {
        Decision::Approved => "message.reported_approval",
        Decision::ApprovedWithSuggestions => "message.approval_with_suggestions",
        Decision::ChangesRequested => "message.changes_requested",
        Decision::Waiting => "message.waiting_for_author",
        Decision::Dismissed => "message.review_dismissed",
        Decision::None => "message.no_vote",
        Decision::Unknown => "message.review_unavailable",
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
            error.set(
                "message.this_pr_mr_connection_changed_reload_connections_and_read_the_li".into(),
            );
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
                error.set("message.invalid_read_parameters".into());
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
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                if report.pull_request.revision != revision
                    || !atelier_core::application::forge::same_pull_request(
                        &pr,
                        &report.pull_request,
                    )
                {
                    return Err(
                        "message.the_pr_mr_changed_refresh_its_link_before_reading_checks_again"
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
                    error.set(
                        "message.the_link_or_credentials_changed_during_the_read_read_the_checks"
                            .into(),
                    );
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
    view! {
        <section
            class="task-files"
            hidden={move || linked.with(|linked| linked.is_none())}
            aria-labelledby="forge-checks-heading"
            aria-busy={move || loading.get().to_string()}
        >
            <h4 id="forge-checks-heading">{move || i18n.text("message.forge_checks_and_reviews")}</h4>
            <p class="field-help">
                {move || {
                    i18n.text("message.states_reported_by_the_forge_without_starting_ci_this_read_valid")
                }}
            </p>
            <crate::button::Button
                disabled={Signal::derive(move || {
                    loading.get() || configuration.with(|configuration| configuration.is_none())
                })}
                {..}
                id="read-forge-checks"
                type="button"
                on:click={inspect}
            >
                {move || i18n.text("message.read_checks_and_reviews")}
            </crate::button::Button>
            <p id="forge-checks-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p role="status">
                {move || {
                    if loading.get() {
                        i18n.text("message.reading")
                    } else {
                        result
                            .with(|result| {
                                result
                                    .as_ref()
                                    .map(|report| {
                                        format!(
                                            "{} · {}",
                                            report.pull_request.source_commit.as_deref().unwrap_or(""),
                                            report.pull_request.checked_at,
                                        )
                                    })
                                    .unwrap_or_default()
                            })
                    }
                }}
            </p>
            <Show when={move || result.with(|result| result.is_some())}>
                <p hidden={move || {
                    count.get() != 0
                }}>{move || i18n.text("message.no_check_observed_for_this_snapshot_no_success_inferred")}</p>
                <div
                    class="table-scroll"
                    tabindex="0"
                    role="region"
                    aria-label={move || i18n.text("message.check_results")}
                >
                    <table class="report-table">
                        <thead>
                            <tr>
                                <th>{move || i18n.text("message.check")}</th>
                                <th>{move || i18n.text("message.reported_state")}</th>
                                <th>{move || i18n.text("message.scope")}</th>
                                <th>{move || i18n.text("message.detail")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {move || {
                                visible
                                    .get()
                                    .into_iter()
                                    .map(|check| {
                                        view! {
                                            <tr>
                                                <td>{check.name}</td>
                                                <td>{i18n.text(state(check.state))}</td>
                                                <td>{i18n.text(scope(check.scope))}</td>
                                                <td>
                                                    {check
                                                        .description_key
                                                        .as_deref()
                                                        .map(|key| i18n.text(key))
                                                        .unwrap_or(check.description)}
                                                </td>
                                            </tr>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </tbody>
                    </table>
                </div>
                <crate::button::Button
                    disabled={Signal::derive(move || page.get() == 0)}
                    {..}
                    type="button"
                    on:click={move |_| page.update(|page| *page = page.saturating_sub(1))}
                >
                    {move || i18n.text("message.previous")}
                </crate::button::Button>
                <crate::button::Button
                    disabled={Signal::derive(move || { (page.get() + 1) * 100 >= count.get() })}
                    {..}
                    type="button"
                    on:click={move |_| page.update(|page| *page += 1)}
                >
                    {move || i18n.text("message.next")}
                </crate::button::Button>
                <p>
                    {move || {
                        result
                            .with(|result| {
                                result
                                    .as_ref()
                                    .map(|report| {
                                        i18n.text("message.checks_for_other_snapshots_excluded_0")
                                            .replace("{0}", &report.other_snapshot_count.to_string())
                                    })
                                    .unwrap_or_default()
                            })
                    }}
                </p>
                <h5>{move || i18n.text("message.reported_human_reviews")}</h5>
                <p hidden={move || {
                    result.with(|result| result.as_ref().is_some_and(|report| report.reviewers_available))
                }}>{move || i18n.text("message.reviews_unavailable_no_absence_of_reviews_inferred")}</p>
                <p hidden={move || {
                    reviewer_count.get() != 0
                        || result
                            .with(|result| result.as_ref().is_none_or(|report| !report.reviewers_available))
                }}>{move || i18n.text("message.no_human_review_observed")}</p>
                <ul>
                    {move || {
                        result
                            .with(|result| {
                                result
                                    .as_ref()
                                    .map(|report| {
                                        report
                                            .reviewers
                                            .iter()
                                            .skip(reviewer_page.get() * 100)
                                            .take(100)
                                            .cloned()
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default()
                            })
                            .into_iter()
                            .map(|reviewer| {
                                view! {
                                    <li>
                                        {reviewer.name}" · "{i18n.text(decision(reviewer.decision))}" · "
                                        {i18n.text(scope(reviewer.scope))}" · "
                                        <code>
                                            {reviewer
                                                .commit
                                                .unwrap_or_else(|| i18n.text("message.commit_not_supplied"))}
                                        </code>
                                    </li>
                                }
                            })
                            .collect_view()
                    }}
                </ul>
                <div role="group" aria-label={move || i18n.text("message.human_review_pages")}>
                    <crate::button::Button
                        disabled={Signal::derive(move || reviewer_page.get() == 0)}
                        {..}
                        type="button"
                        on:click={move |_| reviewer_page.update(|page| *page = page.saturating_sub(1))}
                    >
                        {move || i18n.text("message.previous")}
                    </crate::button::Button>
                    <crate::button::Button
                        disabled={Signal::derive(move || { (reviewer_page.get() + 1) * 100 >= reviewer_count.get() })}
                        {..}
                        type="button"
                        on:click={move |_| reviewer_page.update(|page| *page += 1)}
                    >
                        {move || i18n.text("message.next")}
                    </crate::button::Button>
                </div>
                <ul>
                    {move || {
                        result
                            .with(|result| {
                                result.as_ref().map(|report| report.limits.clone()).unwrap_or_default()
                            })
                            .into_iter()
                            .map(|limit| view! { <li>{i18n.text(&limit)}</li> })
                            .collect_view()
                    }}
                </ul>
            </Show>
        </section>
    }
}
