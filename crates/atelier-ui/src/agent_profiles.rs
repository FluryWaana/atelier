use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::document::Document;
use gloo_net::http::Method;
use leptos::prelude::*;
use std::collections::BTreeMap;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

const FIELDS: [&str; 15] = [
    "id",
    "name",
    "type",
    "auth",
    "model",
    "effort",
    "command",
    "configDir",
    "accountLabel",
    "npmPackage",
    "analyzeArgs",
    "editArgs",
    "versionArgs",
    "statusArgs",
    "protocolArgs",
];

#[derive(Clone, PartialEq)]
struct Draft {
    values: BTreeMap<&'static str, String>,
    key: String,
    clear_key: bool,
}
impl Draft {
    fn new(profile: Option<&Document>) -> Self {
        let mut values = BTreeMap::new();
        for key in FIELDS {
            let value = if key.ends_with("Args") {
                profile
                    .and_then(|profile| profile.raw(key))
                    .map(|raw| raw.get().to_owned())
                    .unwrap_or_else(|| {
                        if key == "versionArgs" {
                            r#"["--version"]"#
                        } else {
                            "[]"
                        }
                        .into()
                    })
            } else {
                profile
                    .map(|profile| field(profile, key))
                    .unwrap_or_default()
            };
            values.insert(key, value);
        }
        if values["type"].is_empty() {
            values.insert(
                "type",
                atelier_core::domain::agent_catalog::AGENTS[0].id.into(),
            );
        }
        if values["auth"].is_empty() {
            values.insert("auth", "cli".into());
        }
        Self {
            values,
            key: String::new(),
            clear_key: false,
        }
    }

    fn body(&self, original: Option<&Document>) -> Result<serde_json::Value, String> {
        let mut body = serde_json::json!({"expected":original, "apiKey":self.key, "clearApiKey":self.clear_key});
        for key in FIELDS {
            if key.ends_with("Args") {
                if (self.values["type"] == "command" && key != "protocolArgs")
                    || (self.values["type"] == "acp" && key == "protocolArgs")
                {
                    let args: Vec<String> = serde_json::from_str(&self.values[key])
                        .map_err(|_| "message.arguments_must_be_a_json_array".to_owned())?;
                    body[key] = serde_json::json!(args);
                }
            } else {
                body[key] =
                    serde_json::json!(if key == "npmPackage" && self.values["type"] != "command" {
                        ""
                    } else {
                        &self.values[key]
                    });
            }
        }
        Ok(body)
    }
}

#[derive(Clone, Copy)]
enum Confirmation {
    Close,
    Reload,
    Remove,
}

async fn catalog(token: &str) -> Result<Vec<Document>, String> {
    read(token, "/api/agent-providers")
        .await?
        .json()
        .await
        .map_err(|_| "message.invalid_server_response".into())
}

pub async fn discover_profiles(token: &str, board: BoardState) -> Result<(Vec<Document>, Vec<atelier_core::contracts::DiscoveredAgent>), String> {
    let mut profiles = catalog(token).await?;
    let agents = read(token, "/api/agent-providers/discover").await?
        .json::<Vec<atelier_core::contracts::DiscoveredAgent>>().await
        .map_err(|_| "message.invalid_server_response".to_owned())?;
    let mut created = false;
    for agent in &agents {
        if !agent.installed || !agent.supported || agent.script_only
            || profiles.iter().any(|profile| field(profile, "type") == agent.id) { continue; }
        let body = serde_json::json!({"expected":null,"id":agent.id,"name":agent.name,"type":agent.id,"auth":"cli","model":"","effort":"","command":"","configDir":"","accountLabel":"","npmPackage":"","apiKey":"","clearApiKey":false});
        match write(token, Method::POST, "/api/agent-providers", &body).await {
            Ok(response) => { profiles = response.json().await.map_err(|_| "message.invalid_server_response".to_owned())?; created = true; },
            Err(message) => {
                profiles = catalog(token).await?;
                if !profiles.iter().any(|profile| field(profile, "type") == agent.id) {
                    return Err(message);
                }
            },
        }
    }
    if created { board.refresh(token).await?; }
    Ok((profiles, agents))
}

#[component]
pub fn AgentProfiles(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let items = RwSignal::new(Vec::<Document>::new());
    let original = RwSignal::new(None::<Document>);
    let baseline = RwSignal::new(Draft::new(None));
    let draft = RwSignal::new(baseline.get_untracked());
    let editing = RwSignal::new(false);
    let dirty = Memo::new(move |_| draft.get() != baseline.get());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let confirmation = RwSignal::new(None::<Confirmation>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let focus = |id: &'static str| {
        request_animation_frame(move || {
            if let Some(element) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(id))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        })
    };
    let select = move |profile: Option<Document>| {
        let next = Draft::new(profile.as_ref());
        baseline.set(next.clone());
        draft.set(next);
        original.set(profile);
        editing.set(true);
        error.set(String::new());
        notice.set(String::new());
        confirmation.set(None);
        focus("profile-name");
    };
    let close = move || {
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
        focus("manage-agents");
    };
    let ask = move |action: Confirmation| {
        confirmation.set(Some(action));
        focus("confirm-profile-action");
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            ask(Confirmation::Close);
        } else {
            close();
        }
    };
    let refresh = move |reload: bool| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let id = draft.get_untracked().values["id"].clone();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = discover_profiles(&token, board).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok((profiles, _)) => {
                    if reload {
                        let current = profiles
                            .iter()
                            .find(|profile| field(profile, "id") == id)
                            .cloned();
                        if current.is_some() {
                            select(current);
                        } else {
                            select(None);
                            editing.set(false);
                            focus("new-profile");
                        }
                    }
                    items.set(profiles);
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    refresh(false);
    let discover = move |_| refresh(false);
    let execute = move |remove: bool| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let id = draft.get_untracked().values["id"].clone();
        let (method, path, body) = if remove {
            (
                Method::DELETE,
                format!("/api/agent-providers/{id}"),
                serde_json::json!({"expected":original.get_untracked()}),
            )
        } else {
            let body = match draft
                .get_untracked()
                .body(original.get_untracked().as_ref())
            {
                Ok(body) => body,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            (Method::POST, "/api/agent-providers".into(), body)
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        confirmation.set(None);
        spawn_local(async move {
            let result = async {
                write(&token, method, &path, &body)
                    .await?
                    .json::<Vec<Document>>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            let _ = board.refresh(&token).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(profiles) => {
                    if remove {
                        select(None);
                        editing.set(false);
                        focus("new-profile");
                    } else {
                        select(
                            profiles
                                .iter()
                                .find(|profile| field(profile, "id") == id)
                                .cloned(),
                        );
                    }
                    items.set(profiles);
                    notice.set(
                        if remove {
                            "message.profile_removed"
                        } else {
                            "message.profile_saved"
                        }
                        .into(),
                    );
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <dialog id="agents-dialog" class="editor" node_ref=dialog aria-labelledby="agents-title" aria-busy=move || pending.get().to_string() on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text("message.agents")}</p><h2 id="agents-title">{move || i18n.text("message.agent_profiles")}</h2></div><button id="close-agents" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("message.close")}</button></header>
            <p class="field-help">{move || i18n.text("message.a_profile_defines_an_agent_and_its_settings_saving_it_does_not_r")}</p>
            <button id="discover-agents" type="button" disabled=move || pending.get() || dirty.get() on:click=discover>{move || i18n.text(if pending.get() { "message.searching_for_agents" } else { "message.find_installed_agents" })}</button>
            <div class="agent-profile-layout">
            <aside class="agent-profile-sidebar" aria-label=move || i18n.text("message.agent_profiles")>
            <div class="editor-actions"><button id="new-profile" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| select(None)>{move || i18n.text("message.new_profile")}</button><button id="refresh-profiles" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| refresh(false)>{move || i18n.text("message.refresh_list")}</button></div>
            <ul class="profile-list"><For each=move || items.get() key=|profile| field(profile,"id") children=move |profile| {
                let id = StoredValue::new(field(&profile,"id"));
                let current = move || items.with(|items| items.iter().find(|profile| field(profile,"id") == id.get_value()).cloned());
                view! { <li><button type="button" data-profile=id.get_value() aria-pressed=move || original.get().is_some_and(|profile| field(&profile,"id") == id.get_value()).to_string() disabled=move || pending.get() || dirty.get() on:click=move |_| select(current())>{move || current().map(|profile| format!("{} · {}", field(&profile,"name"), field(&profile,"type"))).unwrap_or_default()}</button></li> }
            } /></ul>
            <p hidden=move || !dirty.get() class="field-help">{move || i18n.text("message.save_or_reload_this_profile_before_selecting_another_one")}</p>
            </aside>
            <div class="agent-profile-content">
            <p class="agent-profile-empty" hidden=move || editing.get()>{move || i18n.text("message.choose_a_profile_or_create_one_to_get_started")}</p>
            <form hidden=move || !editing.get() on:submit=move |event| { event.prevent_default(); execute(false); }>
                <fieldset disabled=move || pending.get()>
                    <label for="profile-id">{move || i18n.text("message.profile_id")}</label><input id="profile-id" required maxlength="64" pattern="[a-z][a-z0-9-]*" readonly=move || original.get().is_some() prop:value=move || draft.get().values["id"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("id", event_target_value(&event)); }) />
                    <label for="profile-name">{move || i18n.text("message.profile_name")}</label><input id="profile-name" required maxlength="100" prop:value=move || draft.get().values["name"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("name", event_target_value(&event)); }) />
                    <label for="profile-type">{move || i18n.text("message.agent_type")}</label><select id="profile-type" disabled=move || original.get().is_some() prop:value=move || draft.get().values["type"].clone() on:change=move |event| draft.update(|draft| { draft.values.insert("type",event_target_value(&event)); draft.values.insert("auth","cli".into()); draft.values.insert("model",String::new()); draft.values.insert("effort",String::new()); draft.values.insert("configDir",String::new()); draft.key.clear(); draft.clear_key=false; })>{atelier_core::domain::agent_catalog::AGENTS.iter().filter(|agent| agent.supported).map(|agent| view! { <option value=agent.id>{move || match agent.id { "acp" => i18n.text("message.custom_acp_agent"), "command" => i18n.text("message.custom_command"), _ => agent.name.to_owned() }}</option> }).collect_view()}</select>
                    {[ ("command", "message.executable_optional_for_built_in_agents", "2000"), ("configDir", "message.configuration_folder_optional", "2000"), ("accountLabel", "message.account_label_optional", "200") ].into_iter().map(move |(key,label,limit)| { let hidden = move || atelier_core::domain::agent_catalog::find(&draft.get().values["type"]).is_some_and(|agent| agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Acp && key == "configDir" && agent.config_environment.is_none()); view! { <label hidden=hidden for=format!("profile-{key}")>{move || i18n.text(label)}</label><input hidden=hidden id=format!("profile-{key}") maxlength=limit spellcheck="false" prop:value=move || draft.get().values[key].clone() on:input=move |event| draft.update(|draft| { draft.values.insert(key,event_target_value(&event)); }) /> } }).collect_view()}
                    <p class="field-help">{move || i18n.text("message.the_executable_does_not_include_its_arguments_leave_model_and_re")}</p>
                    <div hidden=move || !atelier_core::domain::agent_catalog::find(&draft.get().values["type"]).is_some_and(|agent| agent.api_auth)>
                        <label for="profile-auth">{move || i18n.text("message.authentication")}</label><select id="profile-auth" prop:value=move || draft.get().values["auth"].clone() on:change=move |event| draft.update(|draft| { draft.values.insert("auth",event_target_value(&event)); draft.key.clear(); draft.clear_key=false; })><option value="cli">{move || i18n.text("message.existing_cli_login")}</option><option value="api">{move || i18n.text("message.api_key_through_the_cli")}</option></select>
                        <p class="field-help" hidden=move || draft.get().values["auth"] == "api">{move || i18n.text("message.saving_a_cli_login_removes_this_profile_s_api_key")}</p>
                        <div hidden=move || draft.get().values["auth"] != "api"><label for="profile-key">{move || i18n.text("message.api_key")}</label><input id="profile-key" type="password" autocomplete="new-password" maxlength="4000" disabled=move || draft.get().clear_key prop:value=move || draft.get().key on:input=move |event| draft.update(|draft| draft.key=event_target_value(&event)) />
                            <p id="profile-key-state" class="field-help">{move || i18n.text(if original.get().is_some_and(|profile| profile.get::<bool>("apiKeySet").ok().flatten() == Some(true)) { "message.secret_saved_leave_blank_to_keep_it" } else { "message.no_secret_saved" })}</p>
                            <label class="check-filter"><input id="profile-clear-key" type="checkbox" prop:checked=move || draft.get().clear_key on:change=move |event| draft.update(|draft| { draft.clear_key=event_target_checked(&event); if draft.clear_key { draft.key.clear(); } }) />{move || i18n.text("message.forget_api_key")}</label>
                        </div>
                    </div>
                    <div hidden=move || draft.get().values["type"] != "command">
                        <p class="field-help">{move || i18n.text("message.arguments_are_json_arrays_of_strings_without_an_intermediate_she")}</p>
                        {[("analyzeArgs","message.analysis_arguments"),("editArgs","message.editing_arguments"),("versionArgs","message.version_arguments"),("statusArgs","message.account_status_arguments")].into_iter().map(move |(key,label)| view! { <label for=format!("profile-{key}")>{move || i18n.text(label)}</label><textarea id=format!("profile-{key}") rows="2" maxlength="410000" spellcheck="false" prop:value=move || draft.get().values[key].clone() on:input=move |event| draft.update(|draft| { draft.values.insert(key,event_target_value(&event)); }) /> }).collect_view()}
                        <label for="profile-npmPackage">{move || i18n.text("message.npm_package_for_update_checks_optional")}</label><input id="profile-npmPackage" maxlength="200" prop:value=move || draft.get().values["npmPackage"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("npmPackage",event_target_value(&event)); }) />
                    </div>
                    <div hidden=move || draft.get().values["type"] != "acp">
                        <label for="profile-protocolArgs">{move || i18n.text("message.acp_protocol_arguments_json_array")}</label><textarea id="profile-protocolArgs" rows="2" maxlength="410000" spellcheck="false" prop:value=move || draft.get().values["protocolArgs"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("protocolArgs", event_target_value(&event)); }) />
                    </div>
                </fieldset>
                <div class="editor-actions"><button id="save-profile" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text("message.save")}</button><button id="reload-profile" type="button" disabled=move || pending.get() on:click=move |_| { if dirty.get_untracked() { ask(Confirmation::Reload); } else { refresh(true); } }>{move || i18n.text("message.reload_profile")}</button><button id="remove-profile" type="button" class="danger" hidden=move || original.get().is_none() disabled=move || pending.get() || dirty.get() on:click=move |_| ask(Confirmation::Remove)>{move || i18n.text("message.remove_profile")}</button></div>
            </form>
            <crate::agent_diagnostics::AgentDiagnostics profile=original token pending blocked=dirty />
            <div class="confirmation" hidden=move || confirmation.get().is_none()><p>{move || i18n.text(if matches!(confirmation.get(),Some(Confirmation::Remove)) { "message.remove_this_profile_and_its_key_profiles_used_by_tasks_or_projec" } else { "message.discard_unsaved_changes" })}</p><button id="confirm-profile-action" type="button" class="danger" disabled=move || pending.get() on:click=move |_| { let action=confirmation.get_untracked(); confirmation.set(None); match action { Some(Confirmation::Close)=>close(), Some(Confirmation::Reload)=>refresh(true), Some(Confirmation::Remove)=>execute(true), None=>{} } }>{move || i18n.text("message.confirm")}</button><button id="cancel-profile-action" type="button" disabled=move || pending.get() on:click=move |_| { confirmation.set(None); focus("profile-name"); }>{move || i18n.text("message.cancel")}</button></div>
            <p id="profiles-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="profiles-notice" role="status">{move || i18n.text(&notice.get())}</p>
            </div>
            </div>
        </dialog>
    }
}
