use crate::{i18n::I18n, live::BoardState, write};
use atelier_core::{
    application::extensions::{
        self as protocol, Connection, Method, ModulePullRequestLink, RemoteOperation, Report,
    },
    contracts::Card,
};
use gloo_net::http::Method as Http;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn ConnectorRepositories(
    connection: Signal<Option<Connection>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
    on_prepared: Callback<RemoteOperation>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let selected = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let target = RwSignal::new("main".to_owned());
    let draft = RwSignal::new(true);
    let touched = RwSignal::new(false);
    let report = RwSignal::new(None::<Report>);
    let frozen = RwSignal::new(None::<Card>);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let detach_approved = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let supported = Signal::derive(move || {
        connection.get().is_some_and(|connection| {
            connection
                .installed
                .manifest
                .methods
                .contains(&Method::RepositoriesPreview)
        })
    });
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == selected.get()).cloned())
    });
    let detach_snapshot =
        Memo::new(move |_| serde_json::to_string(&card.get()).unwrap_or_default());
    Effect::new(move |_| {
        let _ = detach_snapshot.get();
        detach_approved.set(false);
    });
    let key = Memo::new(move |_| {
        (
            selected.get(),
            connection.get().map(|connection| {
                (
                    connection.installed.manifest.id,
                    connection.installed.revision,
                )
            }),
        )
    });
    Effect::new(move |_| {
        let _ = key.get();
        if !touched.get_untracked() {
            if let Some(card) = card.get_untracked() {
                title.set(card.title.clone());
                body.set(
                    card.fields
                        .get::<String>("description")
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                );
                target.set(
                    card.fields
                        .get::<ModulePullRequestLink>("connectorPullRequest")
                        .ok()
                        .flatten()
                        .map(|link| link.pull_request.target_branch)
                        .or_else(|| {
                            card.fields
                                .get::<atelier_core::contracts::Worktree>("worktree")
                                .ok()
                                .flatten()
                                .map(|worktree| worktree.base)
                        })
                        .unwrap_or_else(|| "main".into()),
                );
            }
            report.set(None);
            frozen.set(None);
            error.set(String::new());
        }
    });
    Effect::new(move |_| {
        dirty.set(touched.get() || report.get().is_some() || detach_approved.get())
    });
    let stale = Memo::new(move |_| {
        let Some(expected) = frozen.get() else {
            return false;
        };
        card.get().is_none_or(|card| {
            serde_json::to_value(protocol::repository_source(&card)).ok()
                != serde_json::to_value(protocol::repository_source(&expected)).ok()
        })
    });
    let execute = StoredValue::new_local(move |method: Method| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let (Some(connection), Some(expected)) = (connection.get_untracked(), card.get_untracked())
        else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let profile = connection.installed;
        let publishing = method == Method::RepositoriesPublish;
        let (path, input) = if publishing {
            let Some(report) = report
                .get_untracked()
                .filter(|report| report.method == Method::RepositoriesPreview)
            else {
                return;
            };
            if stale.get_untracked() {
                return;
            }
            (
                format!("/api/extensions/{}/operations", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":"repositoriesPublish","input":{"expected":expected,"reportId":report.id}}),
            )
        } else {
            (
                format!("/api/extensions/{}/invoke", profile.manifest.id),
                serde_json::json!({"expectedRevision":profile.revision,"method":method,"action":null,"confirmed":true,"input":{"expected":expected,"target":target.get_untracked(),"title":title.get_untracked(),"body":body.get_untracked(),"draft":draft.get_untracked()}}),
            )
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        let expected_key = key.get_untracked();
        spawn_local(async move {
            let result = async {
                let response = write(&credential, Http::POST, &path, &input).await?;
                if publishing {
                    let operation = response
                        .json::<RemoteOperation>()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())?;
                    Ok::<_, String>((None, Some(operation)))
                } else {
                    let report = response
                        .json::<Report>()
                        .await
                        .map_err(|_| "message.invalid_server_response".to_owned())?;
                    Ok((Some(report), None))
                }
            }
            .await;
            if result.is_ok() {
                let _ = board.refresh(&credential).await;
            }
            if alive.get() {
                pending.set(false);
                if key.get_untracked() == expected_key {
                    match result {
                        Ok((next, operation)) => {
                            if let Some(operation) = operation {
                                on_prepared.run(operation);
                                report.set(None);
                                frozen.set(None);
                                touched.set(false);
                                notice.set("message.dispatch_preview_saved_confirm_sending_it_in_the_journal_below".into());
                            } else {
                                report.set(next);
                                frozen.set(Some(expected));
                            }
                        }
                        Err(message) => error.set(message),
                    }
                }
            }
        });
    });
    let detach = move |_| {
        if !detach_approved.get_untracked() || pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let path = format!("/api/cards/{}/forge/detach", expected.id);
        let alive = mounted.get_value();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(
                &credential,
                Http::POST,
                &path,
                &serde_json::json!({"expected":expected,"confirmed":true}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                detach_approved.set(false);
                match result {
                    Ok(_) => {
                        report.set(None);
                        frozen.set(None);
                        touched.set(false);
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    view! {
        <fieldset hidden={move || !supported.get()} disabled={move || pending.get() || blocked.get()}>
            <legend>{move || i18n.text("message.pr_mr_through_this_module")}</legend>
            <p class="field-help">
                {move || {
                    i18n.text("message.prepare_a_local_commit_and_keep_the_worktree_clean_the_module_ha")
                }}
            </p>
            <crate::form::Field
                id="connector-repository-card"
                label={Signal::derive(move || i18n.text("message.task"))}
            >
                <crate::form::Select
                    {..}
                    id="connector-repository-card"
                    disabled={move || touched.get() || report.get().is_some()}
                    prop:value={move || selected.get()}
                    on:change={move |event| selected.set(event_target_value(&event))}
                >
                    <option value="">{move || i18n.text("message.choose_a_task")}</option>
                    {move || {
                        board
                            .cards
                            .get()
                            .into_iter()
                            .filter(|card| {
                                !card.fields.flag("archived").unwrap_or(true)
                                    && card.fields.raw("worktree").is_some()
                                    && card.fields.raw("pullRequest").is_none()
                            })
                            .map(|card| view! { <option value={card.id}>{card.title.clone()}</option> })
                            .collect_view()
                    }}
                </crate::form::Select>
            </crate::form::Field>
            <crate::form::Field
                id="connector-repository-target"
                label={Signal::derive(move || i18n.text("message.target_branch"))}
            >
                <crate::form::Input
                    {..}
                    id="connector-repository-target"
                    maxlength="180"
                    prop:value={move || target.get()}
                    on:input={move |event| {
                        target.set(event_target_value(&event));
                        touched.set(true);
                        report.set(None);
                    }}
                />
            </crate::form::Field>
            <crate::form::Field
                id="connector-repository-title"
                label={Signal::derive(move || i18n.text("message.title"))}
            >
                <crate::form::Input
                    {..}
                    id="connector-repository-title"
                    maxlength="255"
                    prop:value={move || title.get()}
                    on:input={move |event| {
                        title.set(event_target_value(&event));
                        touched.set(true);
                        report.set(None);
                    }}
                />
            </crate::form::Field>
            <crate::form::Field
                id="connector-repository-body"
                label={Signal::derive(move || i18n.text("message.description"))}
            >
                <crate::form::Textarea
                    {..}
                    id="connector-repository-body"
                    maxlength="3500"
                    prop:value={move || body.get()}
                    on:input={move |event| {
                        body.set(event_target_value(&event));
                        touched.set(true);
                        report.set(None);
                    }}
                ></crate::form::Textarea>
            </crate::form::Field>
            <label>
                <crate::form::Checkbox
                    {..}
                    prop:checked={move || draft.get()}
                    on:change={move |event| {
                        draft.set(event_target_checked(&event));
                        touched.set(true);
                        report.set(None);
                    }}
                />
                {move || i18n.text("message.draft")}
            </label>
            <crate::button::Button
                disabled={Signal::derive(move || card.get().is_none())}
                {..}
                id="preview-connector-repository"
                type="button"
                on:click={move |_| execute.with_value(|execute| execute(Method::RepositoriesPreview))}
            >
                {move || i18n.text("message.prepare_pr_mr_publication")}
            </crate::button::Button>
            <crate::button::Button
                disabled={Signal::derive(move || {
                    card.get().is_none_or(|card| card.fields.raw("connectorPullRequest").is_none())
                })}
                {..}
                id="refresh-connector-repository"
                type="button"
                on:click={move |_| execute.with_value(|execute| execute(Method::RepositoriesRefresh))}
            >
                {move || i18n.text("message.refresh_the_module_pr_mr")}
            </crate::button::Button>
            <label hidden={move || {
                card.get().is_none_or(|card| card.fields.raw("connectorPullRequest").is_none())
            }}>
                <crate::form::Checkbox
                    {..}
                    prop:checked={move || detach_approved.get()}
                    on:change={move |event| detach_approved.set(event_target_checked(&event))}
                />
                {move || i18n.text("message.confirm_removing_this_local_pr_mr_association")}
            </label>
            <crate::button::Button
                disabled={Signal::derive(move || !detach_approved.get())}
                {..}
                type="button"
                on:click={detach}
            >
                {move || i18n.text("message.remove_the_module_pr_mr_association")}
            </crate::button::Button>
            {move || {
                report
                    .get()
                    .and_then(|report| protocol::repository_proposal(&report.output).ok())
                    .map(|proposal| {
                        view! {
                            <dl>
                                <dt>{i18n.text("message.remote_repository")}</dt>
                                <dd>{proposal.observation.reference.origin}</dd>
                                <dt>{i18n.text("message.source_commit")}</dt>
                                <dd>{proposal.context.source}</dd>
                                <dt>{i18n.text("message.target_commit")}</dt>
                                <dd>{proposal.context.target}</dd>
                                <dt>{i18n.text("message.common_ancestor")}</dt>
                                <dd>{proposal.context.base}</dd>
                            </dl>
                        }
                    })
            }}
            <p role="status" hidden={move || !stale.get()}>
                {move || i18n.text("message.the_task_changed_read_its_preview_again_before_confirming")}
            </p>
            <crate::button::Button
                disabled={Signal::derive(move || {
                    stale.get()
                        || report.get().is_none_or(|report| report.method != Method::RepositoriesPreview)
                })}
                {..}
                id="prepare-connector-repository"
                type="button"
                on:click={move |_| execute.with_value(|execute| execute(Method::RepositoriesPublish))}
            >
                {move || i18n.text("message.save_the_pr_mr_dispatch_preview")}
            </crate::button::Button>
            <crate::button::Button
                {..}
                type="button"
                on:click={move |_| {
                    report.set(None);
                    frozen.set(None);
                    touched.set(false);
                    selected.set(String::new());
                    notice.set(String::new());
                }}
            >
                {move || i18n.text("message.cancel_draft")}
            </crate::button::Button>
            <p class="field-help">
                {move || {
                    i18n.text("message.after_publication_start_a_review_from_the_task_runs_revalidate_i")
                }}
            </p>
            <p role="status">{move || i18n.text(&notice.get())}</p>
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
        </fieldset>
    }
}
