use crate::{i18n::I18n, read, write};
use atelier_core::application::updates::{Phase, Status};
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
    let archive = RwSignal::new(String::new());
    let signature = RwSignal::new(String::new());
    let approved = RwSignal::new(false);
    let recovery = RwSignal::new(false);
    let cancel = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let uncertain = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let own_dirty = Memo::new(move |_| {
        !archive.get().is_empty()
            || !signature.get().is_empty()
            || approved.get()
            || recovery.get()
            || cancel.get()
    });
    Effect::new(move |_| dirty.set(own_dirty.get()));
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
        if matches!(action, "apply" | "recover") && uncertain.get_untracked() {
            return;
        }
        if matches!(action, "apply" | "recover") && blocked.get_untracked() {
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
        } else if action == "preview" {
            Some(
                serde_json::json!({"archive":archive.get_untracked(),"signature":signature.get_untracked()}),
            )
        } else {
            let Some(operation) = operation() else {
                return;
            };
            let confirmed = match action {
                "apply" => approved.get_untracked(),
                "recover" => recovery.get_untracked(),
                "cancel" => cancel.get_untracked(),
                _ => false,
            };
            if !confirmed {
                return;
            }
            Some(serde_json::json!({"id":operation.id,"confirmed":true}))
        };
        if action == "reload" {
            loading.set(true);
        } else {
            pending.set(true);
        }
        if matches!(action, "apply" | "recover") {
            uncertain.set(true);
        }
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
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
                read(&credential, "/api/updates")
                    .await?
                    .json::<Status>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(value) => {
                    status.set(Some(value));
                    uncertain.set(false);
                    approved.set(false);
                    recovery.set(false);
                    cancel.set(false);
                    if action == "preview" {
                        archive.set(String::new());
                        signature.set(String::new());
                    }
                }
                Err(message) => error.set(message),
            }
            if matches!(action, "apply" | "recover") {
                notice.set(
                    "message.the_command_may_have_been_received_atelier_restarts_if_it_is_acc"
                        .into(),
                );
                approved.set(false);
                recovery.set(false);
            }
            if action == "reload" {
                loading.set(false);
            } else {
                pending.set(false);
            }
        });
    });
    Effect::new(move |_| execute.with_value(|execute| execute("reload")));
    view! {<section id="updates-settings" aria-labelledby="updates-heading" aria-busy=move||(loading.get()||pending.get()).to_string()>
        <h3 id="updates-heading">{move||i18n.text("message.updates")}</h3>
        <p>{move||i18n.text("message.the_new_version_is_verified_before_activation_the_previous_versi")}</p>
        <p>{move||i18n.text("message.current_version")}" "{move||status.get().map(|status|status.current_version)}</p>
        <p id="system-package-update" hidden=move||!system()>{move||i18n.text("message.update_this_system_installation_using_the_package_manager_after")}</p>
        <p hidden=move||status.get().is_none_or(|status|status.installed)>{move||i18n.text("message.first_install_the_rust_package_with_its_trusted_key_to_use_this")}</p>
        <button id="reload-updates" type="button" disabled=move||pending.get()||loading.get() on:click=move|_|execute.with_value(|execute|execute("reload"))>{move||i18n.text("message.read_update_status_again")}</button>
        <fieldset disabled=move||system()||pending.get()||loading.get()||status.get().is_none_or(|status|!status.installed)||operation().is_some_and(|operation|operation.state.pending())>
            <label for="update-archive">{move||i18n.text("message.update_archive")}</label><input id="update-archive" maxlength="2000" spellcheck="false" prop:value=move||archive.get() on:input=move|event|{archive.set(event_target_value(&event));approved.set(false);}/>
            <label for="update-signature">{move||i18n.text("message.signature_supplied_with_the_archive")}</label><input id="update-signature" maxlength="2000" spellcheck="false" prop:value=move||signature.get() on:input=move|event|{signature.set(event_target_value(&event));approved.set(false);}/>
            <p>{move||i18n.text("message.enter_the_full_paths_of_both_files_on_this_computer_only_this_in")}</p>
            <button id="preview-update" type="button" disabled=move||archive.get().is_empty()||signature.get().is_empty() on:click=move|_|execute.with_value(|execute|execute("preview"))>{move||i18n.text("message.verify_and_prepare_version")}</button>
        </fieldset>
        <div hidden=move||operation().is_none()>
            <h4 id="update-phase">{move||operation().map(|operation|i18n.text(phase_label(operation.state)))}</h4>
            <p>{move||operation().map(|operation|format!("{} → {}",operation.from_version,operation.to_version))}</p>
            <p>{move||operation().map(|operation|i18n.text(&operation.message))}</p>
            <p>{move||i18n.text("message.key_fingerprint")}" "{move||operation().map(|operation|operation.key_id)}</p>
            <p>{move||i18n.text("message.package_fingerprint")}" "{move||operation().map(|operation|operation.sha256)}</p>
            <details><summary>{move||i18n.text("message.prepared_package_contents")}</summary><ul><For each=move||operation().map(|operation|operation.files).unwrap_or_default() key=|file|file.path.clone() children=move|file|view!{<li>{file.path}" · "{file.size}" "{move||i18n.text("message.bytes")}</li>}/></ul></details>
            <p class="field-help" hidden=move||!blocked.get()>{move||i18n.text("message.save_or_discard_other_drafts_before_restarting_atelier")}</p>
            <fieldset hidden=move||operation().is_none_or(|operation|operation.state!=Phase::Prepared) disabled=move||system()||pending.get()||loading.get()||blocked.get()||uncertain.get()>
                <label><input id="approve-update" type="checkbox" prop:checked=move||approved.get() on:change=move|event|approved.set(event_target_checked(&event))/>{move||i18n.text("message.stop_runs_and_services_activate_this_exact_version_and_verify_st")}</label>
                <button id="apply-update" class="primary" type="button" disabled=move||!approved.get() on:click=move|_|execute.with_value(|execute|execute("apply"))>{move||i18n.text("message.update_and_restart")}</button>
            </fieldset>
            <fieldset hidden=move||operation().is_none_or(|operation|!matches!(operation.state,Phase::Preparing|Phase::Prepared)) disabled=move||system()||pending.get()||loading.get()>
                <label><input id="approve-cancel-update" type="checkbox" prop:checked=move||cancel.get() on:change=move|event|cancel.set(event_target_checked(&event))/>{move||i18n.text("message.cancel_this_preparation_without_changing_the_active_version")}</label><button id="cancel-update" type="button" disabled=move||!cancel.get() on:click=move|_|execute.with_value(|execute|execute("cancel"))>{move||i18n.text("message.cancel_preparation")}</button>
            </fieldset>
            <fieldset hidden=move||operation().is_none_or(|operation|!operation.state.maintenance()) disabled=move||system()||pending.get()||loading.get()||blocked.get()||uncertain.get()>
                <label><input id="approve-recover-update" type="checkbox" prop:checked=move||recovery.get() on:change=move|event|recovery.set(event_target_checked(&event))/>{move||i18n.text("message.stop_the_linked_instance_and_try_returning_to_its_verified_previ")}</label><button id="recover-update" type="button" disabled=move||!recovery.get() on:click=move|_|execute.with_value(|execute|execute("recover"))>{move||i18n.text("message.recover_previous_version")}</button>
            </fieldset>
        </div>
        <p id="updates-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="updates-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
