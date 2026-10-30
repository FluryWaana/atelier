use crate::{i18n::I18n, read, write};
use atelier_core::contracts::{ForgeConfiguration, ForgeConnections, ForgeKind, ForgeUpdate};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn ForgeSettings(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let kind = RwSignal::new(ForgeKind::Github);
    let configurations = RwSignal::new(None::<ForgeConnections>);
    let original = RwSignal::new(String::new());
    let base = RwSignal::new(String::new());
    let secret = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let replace = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let own_dirty = Memo::new(move |_| base.get() != original.get() || !secret.get().is_empty());
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let select = move |selected: ForgeKind| {
        let address = configurations.with_untracked(|configurations| {
            configurations
                .as_ref()
                .and_then(|configurations| {
                    configurations
                        .connections
                        .iter()
                        .find(|connection| connection.configuration.kind == selected)
                })
                .map(|connection| connection.configuration.base.clone())
                .unwrap_or_default()
        });
        kind.set(selected);
        original.set(address.clone());
        base.set(address);
        secret.set(String::new());
        replace.set(false);
    };
    let load_alive = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        loading.set(true);
        error.set(String::new());
        replace.set(false);
        let alive = load_alive.clone();
        spawn_local(async move {
            let fetched = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match fetched {
                    Ok(value) => {
                        configurations.set(Some(value));
                        select(kind.get_untracked());
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    Effect::new(move |_| load.with_value(|load| load()));
    let saving = alive.clone();
    let save = StoredValue::new_local(move |remove_token: bool| {
        if pending.get_untracked()
            || loading.get_untracked()
            || (!remove_token && !own_dirty.get_untracked())
        {
            return;
        }
        let Some(configuration) = configurations.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(ForgeUpdate {
            expected_revision: configuration.revision,
            configuration: ForgeConfiguration {
                kind: kind.get_untracked(),
                base: if remove_token {
                    original.get_untracked()
                } else {
                    base.get_untracked()
                },
            },
            token: (!remove_token && !secret.get_untracked().is_empty())
                .then(|| secret.get_untracked()),
            clear_token: remove_token,
        })
        .expect("forge configuration serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = saving.clone();
        spawn_local(async move {
            let saved = async {
                write(&credential, Method::PUT, "/api/forges", &body)
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match saved {
                    Ok(value) => {
                        configurations.set(Some(value));
                        if !remove_token {
                            select(kind.get_untracked());
                        }
                        notice.set(
                            if remove_token {
                                "message.forge_token_removed"
                            } else {
                                "message.forge_connection_saved_no_remote_access_verified"
                            }
                            .into(),
                        );
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    let token_set = Memo::new(move |_| {
        configurations.with(|configurations| {
            configurations
                .as_ref()
                .and_then(|configurations| {
                    configurations
                        .connections
                        .iter()
                        .find(|connection| connection.configuration.kind == kind.get())
                })
                .is_some_and(|connection| connection.token_set)
        })
    });
    view! {
        <section class="forge-settings" aria-labelledby="forges-heading" aria-busy=move||(pending.get()||loading.get()).to_string()>
            <h3 id="forges-heading">{move||i18n.text("message.forge_connections")}</h3>
            <p class="field-help">{move||i18n.text("message.configure_github_gitlab_or_azure_devops_services_then_explicitly")}</p>
            <div class="forge-field">
                <label for="forge-kind">{move||i18n.text("message.forge")}</label>
                <select id="forge-kind" prop:value=move||kind.get().name() disabled=move||pending.get()||loading.get()||own_dirty.get() on:change=move|event|{
                    let selected=match event_target_value(&event).as_str(){"gitlab"=>ForgeKind::Gitlab,"azure"=>ForgeKind::Azure,_=>ForgeKind::Github};
                    select(selected);
                }>
                    <option value="github">"GitHub"</option>
                    <option value="gitlab">"GitLab"</option>
                    <option value="azure">"Azure DevOps Services"</option>
                </select>
            </div>
            <form on:submit=move|event:leptos::ev::SubmitEvent|{event.prevent_default();save.with_value(|save|save(false));}>
                <fieldset disabled=move||configurations.with(|value|value.is_none())||pending.get()||loading.get()>
                    <div class="forge-field">
                        <label for="forge-base">{move||i18n.text("message.site_or_organization_address")}</label>
                        <input id="forge-base" aria-describedby="forge-base-help" type="url" required maxlength="2000" placeholder=move||match kind.get(){ForgeKind::Github=>"https://github.com",ForgeKind::Gitlab=>"https://gitlab.com",ForgeKind::Azure=>"https://dev.azure.com/ORGANISATION"} prop:value=move||base.get() on:input=move|event|base.set(event_target_value(&event)) />
                        <p id="forge-base-help" class="field-help">{move||i18n.text("message.use_the_github_site_root_gitlab_site_prefix_or_azure_organizatio")}</p>
                    </div>
                    <div class="forge-field">
                        <label for="forge-token">{move||i18n.text("message.forge_personal_access_token")}</label>
                        <input id="forge-token" aria-describedby="forge-token-help" type="password" autocomplete="off" maxlength="4000" prop:value=move||secret.get() on:input=move|event|secret.set(event_target_value(&event)) />
                        <p id="forge-token-help" class="field-help" role="status">{move||i18n.text(if token_set.get(){"message.a_token_is_saved_leave_blank_to_keep_it"}else{"message.no_token_saved_for_this_forge"})}</p>
                    </div>
                </fieldset>
                <div class="forge-actions">
                    <button id="forge-clear-token" class="danger" type="button" hidden=move||!token_set.get() disabled=move||pending.get()||loading.get() on:click=move|_|save.with_value(|save|save(true))>{move||i18n.text("message.remove_saved_token")}</button>
                    <div class="forge-save-actions">
                        <button id="reload-forges" type="button" disabled=move||pending.get()||loading.get() on:click=move|_|{
                            if own_dirty.get_untracked(){replace.set(true);}else{load.with_value(|load|load());}
                        }>{move||i18n.text("message.refresh")}</button>
                        <button id="save-forge" class="primary" type="submit" disabled=move||pending.get()||loading.get()||configurations.with(|value|value.is_none())||!own_dirty.get()>{move||i18n.text(if pending.get(){"message.saving"}else{"message.save"})}</button>
                    </div>
                </div>
            </form>
            <div class="confirmation" hidden=move||!replace.get()>
                <p>{move||i18n.text("message.discard_unsaved_changes")}</p>
                <div class="editor-actions">
                    <button id="confirm-reload-forges" type="button" on:click=move|_|load.with_value(|load|load())>{move||i18n.text("message.discard")}</button>
                    <button type="button" on:click=move|_|replace.set(false)>{move||i18n.text("message.keep_editing")}</button>
                </div>
            </div>
            <p class="error" id="forge-error" role="alert">{move||i18n.text(&error.get())}</p>
            <p id="forge-notice" role="status">{move||i18n.text(&notice.get())}</p>
        </section>
    }
}
