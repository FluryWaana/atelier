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

fn next_profile_id(profiles: &[Document]) -> String {
    (1u64..)
        .map(|number| format!("profile-{number}"))
        .find(|id| !profiles.iter().any(|profile| field(profile, "id") == *id))
        .expect("available profile identifier")
}

#[derive(Clone, Copy)]
enum Confirmation {
    Close,
    Cancel,
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

pub async fn discover_profiles(
    token: &str,
    board: BoardState,
) -> Result<(Vec<Document>, Vec<atelier_core::contracts::DiscoveredAgent>), String> {
    let mut profiles = catalog(token).await?;
    let agents = read(token, "/api/agent-providers/discover")
        .await?
        .json::<Vec<atelier_core::contracts::DiscoveredAgent>>()
        .await
        .map_err(|_| "message.invalid_server_response".to_owned())?;
    let mut created = false;
    for agent in &agents {
        if !agent.installed
            || !agent.supported
            || agent.script_only
            || profiles
                .iter()
                .any(|profile| field(profile, "type") == agent.id)
        {
            continue;
        }
        let body = serde_json::json!({"expected":null,"id":agent.id,"name":agent.name,"type":agent.id,"auth":"cli","model":"","effort":"","command":"","configDir":"","accountLabel":"","npmPackage":"","apiKey":"","clearApiKey":false});
        match write(token, Method::POST, "/api/agent-providers", &body).await {
            Ok(response) => {
                profiles = response
                    .json()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                created = true;
            }
            Err(message) => {
                profiles = catalog(token).await?;
                if !profiles
                    .iter()
                    .any(|profile| field(profile, "type") == agent.id)
                {
                    return Err(message);
                }
            }
        }
    }
    if created {
        board.refresh(token).await?;
    }
    Ok((profiles, agents))
}

#[component]
pub fn AgentProfiles(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let statuses = expect_context::<crate::agent_diagnostics::AgentStatuses>();
    let items = RwSignal::new(Vec::<Document>::new());
    let original = RwSignal::new(None::<Document>);
    let baseline = RwSignal::new(Draft::new(None));
    let draft = RwSignal::new(baseline.get_untracked());
    let custom_agent =
        Memo::new(move |_| matches!(draft.get().values["type"].as_str(), "command" | "acp"));
    let editing = RwSignal::new(false);
    let replacing_key = RwSignal::new(false);
    let has_key = Memo::new(move |_| {
        original
            .get()
            .is_some_and(|profile| profile.get::<bool>("apiKeySet").ok().flatten() == Some(true))
    });
    let dirty = Memo::new(move |_| draft.get() != baseline.get());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let confirmation = RwSignal::new(None::<Confirmation>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
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
        let mut next = Draft::new(profile.as_ref());
        if profile.is_none() {
            next.values
                .insert("id", next_profile_id(&items.get_untracked()));
        }
        baseline.set(next.clone());
        draft.set(next);
        original.set(profile);
        editing.set(true);
        replacing_key.set(false);
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
        <crate::modal::Modal
            id="agents-dialog"
            class="editor"
            labelledby="agents-title"
            node_ref={dialog}
            busy={Signal::derive(move || pending.get())}
            on_close={Callback::new(move |()| request_close())}
        >
            <crate::modal::ModalHeader class="inset" title_id="agents-title" title=Signal::derive(move||i18n.text("message.agents")) close_id="close-agents" busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|request_close())/><crate::button::Button
                disabled={Signal::derive(move || pending.get() || dirty.get())}
                {..}
                id="discover-agents"
                type="button"
                on:click={discover}
            >
                        {move || {
                            i18n.text(
                                if pending.get() {
                                    "message.searching_for_agents"
                                } else {
                                    "message.find_installed_agents"
                                },
                            )
                        }}
                    </crate::button::Button>
            <p class="field-help">{move || i18n.text("message.agent_profiles_intro")}</p>
            <div class="agent-profile-layout">
                <aside class="agent-profile-sidebar" aria-label={move || i18n.text("message.agent_profiles")}>
                    <div class="editor-actions">
                        <crate::button::Button
                            disabled={Signal::derive(move || pending.get() || dirty.get())}
                            {..}
                            id="new-profile"
                            type="button"
                            on:click={move |_| select(None)}
                        >
                            {move || i18n.text("message.add_agent_profile")}
                        </crate::button::Button>
                    </div>
                    <ul class="profile-list">
                        <For
                            each={move || items.get()}
                            key={|profile| field(profile, "id")}
                            children={move |profile| {
                                let id = StoredValue::new(field(&profile, "id"));
                                let current = move || {
                                    items
                                        .with(|items| {
                                            items
                                                .iter()
                                                .find(|profile| field(profile, "id") == id.get_value())
                                                .cloned()
                                        })
                                };
                                view! {
                                    <li>
                                        <crate::button::Button
                                            disabled={Signal::derive(move || pending.get() || dirty.get())}
                                            {..}
                                            type="button"
                                            data-profile={id.get_value()}
                                            aria-pressed={move || {
                                                original
                                                    .get()
                                                    .is_some_and(|profile| {
                                                        field(&profile, "id") == id.get_value()
                                                    })
                                                    .to_string()
                                            }}
                                            on:click={move |_| select(current())}
                                        >
                                            <span
                                                class={move || {
                                                    if crate::agent_diagnostics::current_status(
                                                            statuses.0,
                                                            &id.get_value(),
                                                            &board.settings.get(),
                                                        )
                                                        .is_some_and(|status| status.cli_connected())
                                                    {
                                                        "online-dot agent-status-online"
                                                    } else {
                                                        "online-dot offline"
                                                    }
                                                }}
                                                role="img"
                                                aria-label={move || {
                                                    i18n.text(
                                                        if crate::agent_diagnostics::current_status(
                                                                statuses.0,
                                                                &id.get_value(),
                                                                &board.settings.get(),
                                                            )
                                                            .is_some_and(|status| status.cli_connected())
                                                        {
                                                            "message.cli_login_present_service_access_has_not_been_verified"
                                                        } else {
                                                            "message.not_verified"
                                                        },
                                                    )
                                                }}
                                            ></span>
                                            <span>
                                                {move || {
                                                    current()
                                                        .map(|profile| field(&profile, "name"))
                                                        .unwrap_or_default()
                                                }}
                                            </span>
                                        </crate::button::Button>
                                    </li>
                                }
                            }}
                        />
                    </ul>
                    <p hidden={move || !dirty.get()} class="field-help">
                        {move || {
                            i18n.text("message.save_or_reload_this_profile_before_selecting_another_one")
                        }}
                    </p>
                </aside>
                <div class="agent-profile-content">
                    <p class="agent-profile-empty" hidden={move || editing.get()}>
                        {move || i18n.text("message.choose_a_profile_or_create_one_to_get_started")}
                    </p>
                    <crate::form::Form busy=Signal::derive(move||pending.get()) on_submit=Callback::new(move |event: leptos::ev::SubmitEvent| {
                            event.prevent_default();
                            execute(false);
                        }) {..}
                        hidden={move || !editing.get()}

                    >
                        <fieldset disabled={move || pending.get()}>
                            <crate::form::Field
                                id="profile-name"
                                label={Signal::derive(move || i18n.text("message.profile_name"))}
                            >
                                <crate::form::Input
                                    {..}
                                    id="profile-name"
                                    required=true
                                    maxlength="100"
                                    prop:value={move || draft.get().values["name"].clone()}
                                    on:input={move |event| {
                                        draft
                                            .update(|draft| {
                                                draft.values.insert("name", event_target_value(&event));
                                            })
                                    }}
                                />
                            </crate::form::Field>
                            <crate::form::Field id="profile-type" label=Signal::derive(move || i18n.text("message.agent_type"))>
                            <span hidden={move || {
                                original.get().is_none()
                            }}>
                                {move || {
                                    atelier_core::domain::agent_catalog::find(&draft.get().values["type"])
                                        .map(|agent| agent.name)
                                        .unwrap_or_default()
                                }}
                            </span>
                            <crate::form::Select
                                {..}
                                id="profile-type"
                                hidden={move || original.get().is_some()}
                                disabled={move || original.get().is_some()}
                                prop:value={move || draft.get().values["type"].clone()}
                                on:change={move |event| {
                                    draft
                                        .update(|draft| {
                                            draft.values.insert("type", event_target_value(&event));
                                            draft.values.insert("auth", "cli".into());
                                            draft.values.insert("model", String::new());
                                            draft.values.insert("effort", String::new());
                                            draft.values.insert("configDir", String::new());
                                            draft.key.clear();
                                            draft.clear_key = false;
                                        })
                                }}
                            >
                                {atelier_core::domain::agent_catalog::AGENTS
                                    .iter()
                                    .filter(|agent| agent.supported)
                                    .map(|agent| {
                                        view! {
                                            <option value={agent
                                                .id}>
                                                {move || match agent.id {
                                                    "acp" => i18n.text("message.custom_acp_agent"),
                                                    "command" => i18n.text("message.custom_command"),
                                                    _ => agent.name.to_owned(),
                                                }}
                                            </option>
                                        }
                                    })
                                    .collect_view()}
                            </crate::form::Select></crate::form::Field>
                            <div hidden={move || {
                                !atelier_core::domain::agent_catalog::find(&draft.get().values["type"])
                                    .is_some_and(|agent| agent.api_auth)
                            }}>
                                <crate::form::Field
                                    id="profile-auth"
                                    label={Signal::derive(move || i18n.text("message.authentication"))}
                                >
                                    <crate::form::Select
                                        {..}
                                        id="profile-auth"
                                        prop:value={move || draft.get().values["auth"].clone()}
                                        on:change={move |event| {
                                            draft
                                                .update(|draft| {
                                                    draft.values.insert("auth", event_target_value(&event));
                                                    draft.key.clear();
                                                    draft.clear_key = false;
                                                })
                                        }}
                                    >
                                        <option value="cli">
                                            {move || i18n.text("message.existing_cli_login")}
                                        </option>
                                        <option value="api">
                                            {move || i18n.text("message.api_key_through_the_cli")}
                                        </option>
                                    </crate::form::Select>
                                </crate::form::Field>
                                <p
                                    class="field-help"
                                    hidden={move || !has_key.get() || draft.get().values["auth"] == "api"}
                                >
                                    {move || {
                                        i18n.text("message.saving_a_cli_login_removes_this_profile_s_api_key")
                                    }}
                                </p>
                                <div
                                    hidden={move || draft.get().values["auth"] != "api"}
                                    class="profile-key-section"
                                >
                                    <p id="profile-key-state" class="field-help" role="status">
                                        {move || {
                                            i18n.text(
                                                if draft.get().clear_key {
                                                    "message.profile_key_removal_pending"
                                                } else if has_key.get() {
                                                    "message.secret_saved_leave_blank_to_keep_it"
                                                } else {
                                                    "message.no_secret_saved"
                                                },
                                            )
                                        }}
                                    </p>
                                    <div hidden={move || !has_key.get()} class="profile-key-actions">
                                        <crate::button::Button
                                            disabled={Signal::derive(move || draft.get().clear_key)}
                                            {..}
                                            id="replace-profile-key"
                                            type="button"
                                            on:click={move |_| {
                                                replacing_key.set(true);
                                                focus("profile-key");
                                            }}
                                        >
                                            {move || i18n.text("message.replace_profile_key")}
                                        </crate::button::Button>
                                        <crate::button::Button
                                            {..}
                                            id="profile-clear-key"
                                            type="button"
                                            aria-pressed={move || draft.get().clear_key.to_string()}
                                            on:click={move |_| {
                                                draft
                                                    .update(|draft| {
                                                        draft.clear_key = !draft.clear_key;
                                                        draft.key.clear();
                                                    });
                                                replacing_key.set(false);
                                            }}
                                        >
                                            {move || {
                                                i18n.text(
                                                    if draft.get().clear_key {
                                                        "message.cancel_key_removal"
                                                    } else {
                                                        "message.forget_api_key"
                                                    },
                                                )
                                            }}
                                        </crate::button::Button>
                                    </div>
                                    <div hidden={move || {
                                        (has_key.get() && !replacing_key.get()) || draft.get().clear_key
                                    }}>
                                        <crate::form::Field
                                            id="profile-key"
                                            label={Signal::derive(move || i18n.text("message.api_key"))}
                                        >
                                            <crate::form::Input
                                                input_type="password"
                                                reveal=true
                                                disabled={Signal::derive(move || draft.get().clear_key)}
                                                {..}
                                                id="profile-key"
                                                autocomplete="new-password"
                                                spellcheck="false"
                                                maxlength="4000"
                                                aria-describedby="profile-key-state"
                                                prop:value={move || draft.get().key}
                                                on:input={move |event| {
                                                    draft.update(|draft| draft.key = event_target_value(&event))
                                                }}
                                            />
                                        </crate::form::Field>
                                    </div>
                                </div>
                            </div>
                            <details
                                id="profile-advanced"
                                class="profile-advanced"
                                prop:open={move || custom_agent.get()}
                            >
                                <summary>{move || i18n.text("message.profile_advanced")}</summary>
                                <div class="profile-advanced-fields">
                                    <crate::form::Field
                                        id="profile-id"
                                        label={Signal::derive(move || i18n.text("message.profile_id"))}
                                    >
                                        <crate::form::Input
                                            {..}
                                            id="profile-id"
                                            required=true
                                            maxlength="64"
                                            pattern="[a-z][a-z0-9-]*"
                                            readonly={move || original.get().is_some()}
                                            prop:value={move || draft.get().values["id"].clone()}
                                            on:input={move |event| {
                                                draft
                                                    .update(|draft| {
                                                        draft.values.insert("id", event_target_value(&event));
                                                    })
                                            }}
                                        />
                                    </crate::form::Field>
                                    {[
                                        (
                                            "command",
                                            "message.executable_optional_for_built_in_agents",
                                            "2000",
                                        ),
                                        ("configDir", "message.configuration_folder_optional", "2000"),
                                        ("accountLabel", "message.account_label_optional", "200"),
                                    ]
                                        .into_iter()
                                        .map(move |(key, label, limit)| {
                                            let hidden = move || {
                                                atelier_core::domain::agent_catalog::find(
                                                        &draft.get().values["type"],
                                                    )
                                                    .is_some_and(|agent| {
                                                        agent.protocol
                                                            == atelier_core::domain::agent_catalog::ProtocolKind::Acp
                                                            && key == "configDir" && agent.config_environment.is_none()
                                                    })
                                            };
                                            view! {
                                                <crate::form::Field id={format!("profile-{key}")} label=Signal::derive(move || i18n.text(label))>
                                                <crate::form::Input
                                                    {..}
                                                    hidden={hidden}
                                                    id={format!("profile-{key}")}
                                                    maxlength={limit}
                                                    required={move || key == "command" && custom_agent.get()}
                                                    spellcheck="false"
                                                    prop:value={move || draft.get().values[key].clone()}
                                                    on:input={move |event| {
                                                        draft
                                                            .update(|draft| {
                                                                draft.values.insert(key, event_target_value(&event));
                                                            })
                                                    }}
                                                /></crate::form::Field>
                                            }
                                        })
                                        .collect_view()}
                                    <p class="field-help">
                                        {move || {
                                            i18n.text(
                                                "message.the_executable_does_not_include_its_arguments_leave_model_and_re",
                                            )
                                        }}
                                    </p>
                                    <div hidden={move || draft.get().values["type"] != "command"}>
                                        <p class="field-help">
                                            {move || {
                                                i18n.text(
                                                    "message.arguments_are_json_arrays_of_strings_without_an_intermediate_she",
                                                )
                                            }}
                                        </p>
                                        {[
                                            ("analyzeArgs", "message.analysis_arguments"),
                                            ("editArgs", "message.editing_arguments"),
                                            ("versionArgs", "message.version_arguments"),
                                            ("statusArgs", "message.account_status_arguments"),
                                        ]
                                            .into_iter()
                                            .map(move |(key, label)| {
                                                view! {
                                                    <crate::form::Field id={format!(
                                                        "profile-{key}",
                                                    )} label=Signal::derive(move || i18n.text(label))>
                                                    <crate::form::Textarea
                                                        {..}
                                                        id={format!("profile-{key}")}
                                                        rows="2"
                                                        maxlength="410000"
                                                        spellcheck="false"
                                                        prop:value={move || draft.get().values[key].clone()}
                                                        on:input={move |event| {
                                                            draft
                                                                .update(|draft| {
                                                                    draft.values.insert(key, event_target_value(&event));
                                                                })
                                                        }}
                                                    /></crate::form::Field>
                                                }
                                            })
                                            .collect_view()}
                                        <crate::form::Field id="profile-npmPackage" label=Signal::derive(move || {
                                                i18n.text("message.npm_package_for_update_checks_optional")
                                            })>
                                        <crate::form::Input
                                            {..}
                                            id="profile-npmPackage"
                                            maxlength="200"
                                            prop:value={move || draft.get().values["npmPackage"].clone()}
                                            on:input={move |event| {
                                                draft
                                                    .update(|draft| {
                                                        draft
                                                            .values
                                                            .insert("npmPackage", event_target_value(&event));
                                                    })
                                            }}
                                        /></crate::form::Field>
                                    </div>
                                    <div hidden={move || draft.get().values["type"] != "acp"}>
                                        <crate::form::Field
                                            id="profile-protocolArgs"
                                            label={Signal::derive(move || {
                                                i18n.text("message.acp_protocol_arguments_json_array")
                                            })}
                                        >
                                            <crate::form::Textarea
                                                {..}
                                                id="profile-protocolArgs"
                                                rows="2"
                                                maxlength="410000"
                                                spellcheck="false"
                                                prop:value={move || {
                                                    draft.get().values["protocolArgs"].clone()
                                                }}
                                                on:input={move |event| {
                                                    draft
                                                        .update(|draft| {
                                                            draft
                                                                .values
                                                                .insert("protocolArgs", event_target_value(&event));
                                                        })
                                                }}
                                            />
                                        </crate::form::Field>
                                    </div>
                                    <crate::button::Button
                                        disabled={Signal::derive(move || pending.get())}
                                        {..}
                                        id="reload-profile"
                                        type="button"
                                        on:click={move |_| {
                                            if dirty.get_untracked() {
                                                ask(Confirmation::Reload);
                                            } else {
                                                refresh(true);
                                            }
                                        }}
                                    >
                                        {move || i18n.text("message.reload_profile")}
                                    </crate::button::Button>
                                </div>
                            </details>
                        </fieldset>
                        <div class="editor-actions">
                            <crate::button::Button
                                variant={crate::button::ButtonVariant::Primary}
                                disabled={Signal::derive(move || pending.get() || !dirty.get())}
                                class="primary"
                                {..}
                                id="save-profile"
                                type="submit"
                            >
                                {move || i18n.text("message.save")}
                            </crate::button::Button>
                            <crate::button::Button
                                disabled={Signal::derive(move || pending.get() || !dirty.get())}
                                {..}
                                id="cancel-profile"
                                type="button"
                                on:click={move |_| ask(Confirmation::Cancel)}
                            >
                                {move || i18n.text("message.cancel_profile_changes")}
                            </crate::button::Button>
                            <crate::button::Button
                                variant={crate::button::ButtonVariant::Danger}
                                disabled={Signal::derive(move || pending.get() || dirty.get())}
                                class="danger"
                                {..}
                                id="remove-profile"
                                type="button"
                                hidden={move || original.get().is_none()}
                                on:click={move |_| ask(Confirmation::Remove)}
                            >
                                {move || i18n.text("message.remove_profile")}
                            </crate::button::Button>
                        </div>
                    </crate::form::Form>
                    <crate::agent_diagnostics::AgentDiagnostics
                        profile={original}
                        token
                        pending
                        blocked={dirty}
                    />
                    <crate::modal::Confirmation error=Signal::derive(move||i18n.text(&error.get())) open=Signal::derive(move||!(confirmation.get().is_none())) busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|{
                                confirmation.set(None);
                                focus("profile-name");
                            })>
                        <p>
                            {move || {
                                i18n.text(
                                    if matches!(confirmation.get(), Some(Confirmation::Remove)) {
                                        "message.remove_this_profile_and_its_key_profiles_used_by_tasks_or_projec"
                                    } else {
                                        "message.discard_unsaved_changes"
                                    },
                                )
                            }}
                        </p>
                        <crate::modal::ModalActions><crate::button::Button
                            disabled={Signal::derive(move || pending.get())}
                            {..}
                            data-confirmation-cancel=""
                            autofocus=true
                            id="cancel-profile-action"
                            type="button"
                            on:click={move |_| {
                                confirmation.set(None);
                                focus("profile-name");
                            }}
                        >
                            {move || i18n.text("message.cancel")}
                        </crate::button::Button><crate::button::Button
                            variant={crate::button::ButtonVariant::Danger}
                            disabled={Signal::derive(move || pending.get())}
                            class="danger"
                            {..}
                            id="confirm-profile-action"
                            type="button"
                            on:click={move |_| {
                                let action = confirmation.get_untracked();
                                confirmation.set(None);
                                match action {
                                    Some(Confirmation::Close) => close(),
                                    Some(Confirmation::Cancel) => select(original.get_untracked()),
                                    Some(Confirmation::Reload) => refresh(true),
                                    Some(Confirmation::Remove) => execute(true),
                                    None => {}
                                }
                            }}
                        >
                            {move || i18n.text("message.confirm")}
                        </crate::button::Button></crate::modal::ModalActions>
                    </crate::modal::Confirmation>
                    <p id="profiles-error" class="error" role="alert">
                        {move || i18n.text(&error.get())}
                    </p>
                    <p id="profiles-notice" role="status">
                        {move || i18n.text(&notice.get())}
                    </p>
                </div>
            </div>
        </crate::modal::Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_allow_multiple_profiles_for_the_same_agent() {
        let profiles = vec![
            serde_json::from_str::<Document>(r#"{"id":"profile-1","type":"claude"}"#).unwrap(),
            serde_json::from_str::<Document>(r#"{"id":"profile-3","type":"claude"}"#).unwrap(),
        ];
        assert_eq!(next_profile_id(&[]), "profile-1");
        assert_eq!(next_profile_id(&profiles), "profile-2");
    }

    #[test]
    fn editing_a_profile_preserves_its_key_until_explicitly_changed() {
        let profile = serde_json::from_str::<Document>(
            r#"{"id":"claude-work","name":"Work","type":"claude","auth":"api","apiKeySet":true}"#,
        )
        .unwrap();
        let mut draft = Draft::new(Some(&profile));
        draft.values.insert("name", "Renamed".into());
        let body = draft.body(Some(&profile)).unwrap();
        assert_eq!(body["apiKey"], "");
        assert_eq!(body["clearApiKey"], false);
        assert_eq!(body["id"], "claude-work");
        draft.key = "synthetic-replacement".into();
        assert_eq!(
            draft.body(Some(&profile)).unwrap()["apiKey"],
            "synthetic-replacement"
        );
        draft.key.clear();
        draft.clear_key = true;
        assert_eq!(draft.body(Some(&profile)).unwrap()["clearApiKey"], true);
    }
}
