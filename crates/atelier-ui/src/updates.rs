use crate::{i18n::I18n, read, write};
use atelier_core::application::updates::{Phase, Release, Status};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Preparing => "message.preparing_update",
        Phase::Prepared => "message.version_ready_for_confirmation",
        Phase::Waiting => "message.stopping_safely",
        Phase::Activating => "message.switching_version",
        Phase::Trying => "message.checking_startup",
        Phase::Committed => "message.update_verified",
        Phase::RollingBack => "message.recovering",
        Phase::RolledBack => "message.previous_version_restored",
        Phase::Cancelled => "message.preparation_cancelled",
        Phase::Failed => "message.preparation_interrupted",
        Phase::Uncertain => "message.recovery_needs_attention",
    }
}

#[component]
pub fn Updates(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let status = RwSignal::new(None::<Status>);
    let release = RwSignal::new(None::<Release>);
    let checked = RwSignal::new(false);

    let loading = RwSignal::new(false);
    let uncertain = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    Effect::new(move |_| dirty.set(pending.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let operation = move || status.get().and_then(|status| status.operation);
    let system = move || status.get().is_some_and(|status| status.system_managed);
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        if action != "reload" && system() {
            error.set(
                "message.update_this_system_installation_using_the_package_manager_after".into(),
            );
            return;
        }
        if matches!(action, "install" | "apply" | "recover") && uncertain.get_untracked() {
            return;
        }
        if matches!(action, "install" | "apply" | "recover") && blocked.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = if action == "reload" {
            None
        } else if action == "install" {
            let Some(release) = release.get_untracked().filter(|release| release.compatible) else {
                return;
            };
            Some(serde_json::json!({"version":release.version}))
        } else {
            let Some(operation) = operation() else {
                return;
            };
            if !matches!(action, "apply" | "recover" | "cancel") {
                return;
            }
            Some(serde_json::json!({"id":operation.id,"confirmed":true}))
        };
        if action == "reload" {
            loading.set(true);
            checked.set(false);
            release.set(None);
        } else {
            pending.set(true);
        }
        if matches!(action, "install" | "apply" | "recover") {
            uncertain.set(true);
        }
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            if action == "install" {
                let alive = alive.clone();
                let credential = credential.clone();
                spawn_local(async move {
                    while alive.get() && pending.get_untracked() {
                        let delay = js_sys::Promise::new(&mut |resolve, _| {
                            set_timeout(
                                move || {
                                    let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
                                },
                                std::time::Duration::from_millis(500),
                            );
                        });
                        let _ = wasm_bindgen_futures::JsFuture::from(delay).await;
                        if !alive.get() || !pending.get_untracked() {
                            break;
                        }
                        if let Ok(response) = read(&credential, "/api/updates").await
                            && let Ok(value) = response.json::<Status>().await
                            && alive.get()
                            && pending.get_untracked()
                        {
                            status.set(Some(value));
                        }
                    }
                });
            }
            let result = async {
                if let Some(body) = body {
                    write(
                        &credential,
                        Method::POST,
                        &format!("/api/updates/{action}"),
                        &body,
                    )
                    .await?;
                }
                let current = read(&credential, "/api/updates")
                    .await?
                    .json::<Status>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                if alive.get() {
                    status.set(Some(current.clone()));
                    uncertain.set(false);
                }
                let latest = if action == "reload" {
                    Some(
                        read(&credential, "/api/updates/check")
                            .await?
                            .json::<Option<Release>>()
                            .await
                            .map_err(|_| "message.invalid_server_response".to_owned())?,
                    )
                } else {
                    None
                };
                Ok::<_, String>((current, latest))
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok((value, latest)) => {
                    if let Some(latest) = latest {
                        release.set(latest);
                        checked.set(true);
                    }
                    status.set(Some(value));
                    uncertain.set(false);
                }
                Err(message) => error.set(message),
            }
            if matches!(action, "install" | "apply" | "recover") {
                notice.set(
                    "message.the_command_may_have_been_received_atelier_restarts_if_it_is_acc"
                        .into(),
                );
            }
            if action == "reload" {
                loading.set(false);
            } else {
                pending.set(false);
            }
        });
    });
    Effect::new(move |_| execute.with_value(|execute| execute("reload")));
    view! {
        <section
            id="updates-settings"
            aria-labelledby="updates-heading"
            aria-busy={move || (loading.get() || pending.get()).to_string()}
        >
            <h3 id="updates-heading">{move || i18n.text("message.updates")}</h3>
            <p>
                {move || {
                    i18n.text("message.the_new_version_is_verified_before_activation_the_previous_versi")
                }}
            </p>
            <p>{move || i18n.text("message.github_update_backup")}</p>
            <p>
                {move || i18n.text("message.current_version")}" "
                {move || status.get().map(|status| status.current_version)}
            </p>
            <p id="system-package-update" hidden={move || !system()}>
                {move || i18n.text("message.update_this_system_installation_using_the_package_manager_after")}
            </p>
            <p hidden={move || {
                status.get().is_none_or(|status| status.installed)
            }}>
                {move || i18n.text("message.first_install_the_rust_package_with_its_trusted_key_to_use_this")}
            </p>
            <button
                id="reload-updates"
                type="button"
                disabled={move || pending.get() || loading.get()}
                on:click={move |_| execute.with_value(|execute| execute("reload"))}
            >
                {move || i18n.text("message.read_update_status_again")}
            </button>
            <p role="status" hidden={move || !loading.get()}>
                {move || i18n.text("message.github_update_checking")}
            </p>
            <div class="update-progress" hidden={move || !pending.get()}>
                <label for="update-download-progress">
                    {move || {
                        i18n
                            .text(
                                if status
                                    .get()
                                    .is_some_and(|status| {
                                        status
                                            .download
                                            .is_some_and(|download| download.received == download.total)
                                    })
                                {
                                    "message.github_update_verifying"
                                } else {
                                    "message.github_update_installing"
                                },
                            )
                    }}
                </label>
                <progress
                    id="update-download-progress"
                    max="100"
                    value={move || {
                        status
                            .get()
                            .and_then(|status| status.download)
                            .map(|download| {
                                if download.total == 0 {
                                    0.0
                                } else {
                                    download.received as f64 / download.total as f64 * 100.0
                                }
                            })
                    }}
                    aria-label={move || i18n.text("message.github_update_installing")}
                ></progress>
                <p class="update-download-size">
                    {move || {
                        status
                            .get()
                            .and_then(|status| status.download)
                            .map(|download| {
                                format!(
                                    "{:.1} / {:.1} {}",
                                    download.received as f64 / 1_048_576.0,
                                    download.total as f64 / 1_048_576.0,
                                    i18n.text("message.github_update_mib"),
                                )
                            })
                    }}
                </p>
            </div>
            <p class="field-help" hidden={move || !blocked.get()}>
                {move || i18n.text("message.save_or_discard_other_drafts_before_restarting_atelier")}
            </p>
            <p hidden={move || {
                !checked.get() || release.get().is_some()
            }}>{move || i18n.text("message.github_update_current")}</p>
            <div hidden={move || release.get().is_none()}>
                <p>
                    {move || i18n.text("message.github_update_available")}" "
                    {move || release.get().map(|release| release.version)}
                </p>
                <p hidden={move || {
                    release.get().is_none_or(|release| release.compatible)
                }}>{move || i18n.text("message.github_update_incompatible")}</p>
                <p>{move || i18n.text("message.github_update_restart")}</p>
                <button
                    id="install-update"
                    class="primary"
                    type="button"
                    disabled={move || {
                        system() || pending.get() || loading.get() || blocked.get() || uncertain.get()
                            || status.get().is_none_or(|status| !status.installed)
                            || release.get().is_none_or(|release| !release.compatible)
                            || operation().is_some_and(|operation| operation.state.pending())
                    }}
                    on:click={move |_| execute.with_value(|execute| execute("install"))}
                >
                    {move || {
                        i18n
                            .text(
                                if pending.get() {
                                    "message.github_update_installing"
                                } else {
                                    "message.github_update_install"
                                },
                            )
                    }}
                </button>
            </div>
            <div hidden={move || operation().is_none()}>
                <h4 id="update-phase">
                    {move || operation().map(|operation| i18n.text(phase_label(operation.state)))}
                </h4>
                <p>
                    {move || {
                        operation()
                            .map(|operation| {
                                format!(
                                    "{}{}{}",
                                    operation.from_version,
                                    i18n.text("message.to_separator"),
                                    operation.to_version,
                                )
                            })
                    }}
                </p>
                <p>{move || operation().map(|operation| i18n.text(&operation.message))}</p>
                <details>
                    <summary>{move || i18n.text("message.prepared_package_contents")}</summary>
                    <p>
                        {move || i18n.text("message.key_fingerprint")}" "
                        {move || operation().map(|operation| operation.key_id)}
                    </p>
                    <p>
                        {move || i18n.text("message.package_fingerprint")}" "
                        {move || operation().map(|operation| operation.sha256)}
                    </p>
                    <ul>
                        <For
                            each={move || operation().map(|operation| operation.files).unwrap_or_default()}
                            key={|file| file.path.clone()}
                            children={move |file| {
                                view! {
                                    <li>
                                        {file.path}" · "{file.size}" "{move || i18n.text("message.bytes")}
                                    </li>
                                }
                            }}
                        />
                    </ul>
                </details>
                <fieldset
                    hidden={move || operation().is_none_or(|operation| operation.state != Phase::Prepared)}
                    disabled={move || {
                        system() || pending.get() || loading.get() || blocked.get() || uncertain.get()
                    }}
                >

                    <button
                        id="apply-update"
                        class="primary"
                        type="button"
                        on:click={move |_| execute.with_value(|execute| execute("apply"))}
                    >
                        {move || i18n.text("message.update_and_restart")}
                    </button>
                </fieldset>
                <fieldset
                    hidden={move || {
                        operation()
                            .is_none_or(|operation| {
                                !matches!(operation.state, Phase::Preparing | Phase::Prepared)
                            })
                    }}
                    disabled={move || system() || pending.get() || loading.get()}
                >
                    <button
                        id="cancel-update"
                        type="button"
                        on:click={move |_| execute.with_value(|execute| execute("cancel"))}
                    >
                        {move || i18n.text("message.cancel_preparation")}
                    </button>
                </fieldset>
                <fieldset
                    hidden={move || operation().is_none_or(|operation| !operation.state.maintenance())}
                    disabled={move || {
                        system() || pending.get() || loading.get() || blocked.get() || uncertain.get()
                    }}
                >
                    <button
                        id="recover-update"
                        type="button"
                        on:click={move |_| execute.with_value(|execute| execute("recover"))}
                    >
                        {move || i18n.text("message.recover_previous_version")}
                    </button>
                </fieldset>
            </div>
            <p id="updates-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="updates-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
        </section>
    }
}
