use super::*;

#[derive(Default)]
pub(super) struct Setup {
    options: Value,
    modes: Value,
    stage: u8,
    expected: Option<(String, String)>,
}

pub(super) fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"elicitation":{"form":{}}},"clientInfo":{"name":"atelier-kanban","version":env!("CARGO_PKG_VERSION")}}})
}

fn form_questions(schema: &Value) -> Result<Vec<AgentQuestion>> {
    let object = schema.as_object().ok_or("message.acp_form_invalid")?;
    if schema["type"] != "object"
        || object
            .keys()
            .any(|key| !["type", "properties", "required"].contains(&key.as_str()))
    {
        return Err("message.acp_form_unsupported");
    }
    let properties = schema["properties"]
        .as_object()
        .filter(|properties| !properties.is_empty() && properties.len() <= 8)
        .ok_or("message.acp_form_invalid")?;
    let required = match schema.get("required") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or("message.acp_form_invalid")?
            .iter()
            .map(|value| value.as_str().ok_or("message.acp_form_invalid"))
            .collect::<Result<Vec<_>>>()?,
    };
    if required.iter().any(|key| !properties.contains_key(*key))
        || required
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != required.len()
    {
        return Err("message.acp_form_invalid");
    }
    properties
        .iter()
        .enumerate()
        .map(|(index, (key, field))| {
            text(&json!(key), 128)?;
            let definition = field.as_object().ok_or("message.acp_field_invalid")?;
            // shortcut: decline formats and complex choices until their validation is supported.
            if definition.keys().any(|key| {
                ![
                    "type",
                    "title",
                    "description",
                    "enum",
                    "minLength",
                    "maxLength",
                    "minimum",
                    "maximum",
                ]
                .contains(&key.as_str())
            }) {
                return Err("message.acp_field_unsupported");
            }
            let kind = field["type"]
                .as_str()
                .filter(|kind| ["string", "integer", "number", "boolean"].contains(kind))
                .ok_or("message.acp_field_unsupported")?;
            for key in ["minLength", "maxLength"] {
                if field
                    .get(key)
                    .is_some_and(|value| kind != "string" || value.as_u64().is_none())
                {
                    return Err("message.acp_constraint_invalid");
                }
            }
            for key in ["minimum", "maximum"] {
                if field.get(key).is_some_and(|value| match kind {
                    "integer" => value.as_i64().is_none(),
                    "number" => value.as_f64().is_none(),
                    _ => true,
                }) {
                    return Err("message.acp_constraint_invalid");
                }
            }
            let label = field
                .get("title")
                .map(|title| text(title, 1000))
                .transpose()?
                .unwrap_or_else(|| key.to_owned());
            let description = field
                .get("description")
                .map(|description| text(description, 2000))
                .transpose()?
                .unwrap_or_default();
            let options = if let Some(values) = field.get("enum") {
                if kind != "string" {
                    return Err("message.acp_choice_unsupported");
                }
                values
                    .as_array()
                    .filter(|values| !values.is_empty() && values.len() <= 20)
                    .ok_or("message.acp_choices_invalid")?
                    .iter()
                    .map(|value| text(value, 1000))
                    .collect::<Result<Vec<_>>>()?
            } else if kind == "boolean" {
                vec!["true".into(), "false".into()]
            } else {
                Vec::new()
            };
            if options
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != options.len()
            {
                return Err("message.acp_choices_duplicate");
            }
            let hint = match kind {
                "integer" => "nombre entier",
                "number" => "nombre",
                "boolean" => "true / false",
                _ => "texte",
            };
            Ok(AgentQuestion {
                id: index.to_string(),
                question: format!(
                    "{label} ({hint}){}",
                    if description.is_empty() {
                        String::new()
                    } else {
                        format!(" — {description}")
                    }
                ),
                options,
                multi_select: false,
                is_secret: false,
                required: required.contains(&key.as_str()),
            })
        })
        .collect()
}

pub(super) fn elicitation_reply(
    pending: &Pending,
    answer: &AgentAnswer,
    deny: bool,
) -> Result<Value> {
    if deny {
        return Ok(json!({"jsonrpc":"2.0","id":pending.wire_id,"result":{"action":"decline"}}));
    }
    let properties = pending.input["requestedSchema"]["properties"]
        .as_object()
        .ok_or("message.acp_form_invalid")?;
    let mut content = serde_json::Map::new();
    for (index, (key, field)) in properties.iter().enumerate() {
        let value = answer
            .answers
            .get(&index.to_string())
            .and_then(|values| values.first())
            .ok_or("message.acp_answer_missing")?;
        if value.is_empty() && !pending.view.questions[index].required {
            continue;
        }
        let parsed = match field["type"].as_str() {
            Some("string") => {
                let length = value.chars().count() as u64;
                if field["minLength"].as_u64().is_some_and(|min| length < min)
                    || field["maxLength"].as_u64().is_some_and(|max| length > max)
                {
                    return Err("message.acp_length_invalid");
                }
                if let Some(options) = field["enum"].as_array()
                    && !options.iter().any(|option| option.as_str() == Some(value))
                {
                    return Err("message.acp_choice_invalid");
                }
                json!(value)
            }
            Some("boolean") => json!(
                value
                    .parse::<bool>()
                    .map_err(|_| "message.acp_boolean_invalid")?
            ),
            Some("integer") => json!(
                value
                    .parse::<i64>()
                    .map_err(|_| "message.acp_integer_invalid")?
            ),
            Some("number") => {
                let number = value
                    .parse::<f64>()
                    .map_err(|_| "message.acp_number_invalid")?;
                if !number.is_finite() {
                    return Err("message.acp_number_invalid");
                }
                json!(number)
            }
            _ => return Err("message.acp_field_unsupported"),
        };
        let out_of_range = if field["type"] == "integer" {
            let number = parsed.as_i64().ok_or("message.acp_integer_invalid")?;
            field["minimum"].as_i64().is_some_and(|min| number < min)
                || field["maximum"].as_i64().is_some_and(|max| number > max)
        } else if field["type"] == "number" {
            let number = parsed.as_f64().ok_or("message.acp_number_invalid")?;
            field["minimum"].as_f64().is_some_and(|min| number < min)
                || field["maximum"].as_f64().is_some_and(|max| number > max)
        } else {
            false
        };
        if out_of_range {
            return Err("message.acp_number_out_of_bounds");
        }
        content.insert(key.clone(), parsed);
    }
    Ok(json!({"jsonrpc":"2.0","id":pending.wire_id,"result":{"action":"accept","content":content}}))
}

fn choice(option: &Value, matches: impl Fn(&str) -> bool) -> Option<&str> {
    for item in option["options"].as_array()? {
        if let Some(value) = item["value"].as_str().filter(|value| matches(value)) {
            return Some(value);
        }
        for child in item["options"].as_array().into_iter().flatten() {
            if let Some(value) = child["value"].as_str().filter(|value| matches(value)) {
                return Some(value);
            }
        }
    }
    None
}

impl Protocol {
    pub(super) fn acp(&mut self, event: Value) -> Result<Effects> {
        if event["jsonrpc"] != "2.0" {
            return Err("message.invalid_acp_message");
        }
        let mut effects = Effects::default();
        let method = event["method"].as_str().unwrap_or("");
        if !method.is_empty() && event.get("id").is_some() {
            let id = wire_id(&event["id"])?;
            if method == "elicitation/create" {
                let input = &event["params"];
                if input["mode"] != "form" {
                    effects.send.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Interaction ACP non prise en charge."}}));
                    return Ok(effects);
                }
                let session_scope = input["sessionId"].as_str() == self.thread.as_deref()
                    && input.get("sessionId").is_some()
                    && input.get("requestId").is_none();
                let request_scope =
                    input["requestId"] == "prompt" && input.get("sessionId").is_none();
                if self.phase != Phase::Running || !(session_scope || request_scope) {
                    return Err("message.acp_request_does_not_belong_to_this_session");
                }
                let message = text(&input["message"], 4000)?;
                let questions = match form_questions(&input["requestedSchema"]) {
                    Ok(questions) => questions,
                    Err(_) => {
                        effects
                            .send
                            .push(json!({"jsonrpc":"2.0","id":id,"result":{"action":"decline"}}));
                        return Ok(effects);
                    }
                };
                self.ask(
                    id,
                    method,
                    input.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: String::new(),
                        title_key: Some("message.the_agent_needs_your_reply".into()),
                        detail: message,
                        secret_warning: true,
                        can_allow: true,
                        questions,
                    },
                    true,
                )?;
                return Ok(effects);
            }
            if method != "session/request_permission" {
                effects.send.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Méthode non prise en charge."}}));
                return Ok(effects);
            }
            if self.phase != Phase::Running
                || event["params"]["sessionId"].as_str() != self.thread.as_deref()
            {
                return Err("message.acp_request_does_not_belong_to_this_session");
            }
            let input = event["params"].clone();
            if !input["toolCall"].is_object() {
                return Err("message.acp_request_has_no_tool_details");
            }
            let title = text(&input["toolCall"]["title"], 1000).ok();
            let options = input["options"]
                .as_array()
                .filter(|options| !options.is_empty() && options.len() <= 20)
                .ok_or("message.invalid_acp_options")?;
            let mut ids = std::collections::BTreeSet::new();
            for option in options {
                if !ids.insert(text(&option["optionId"], 128)?) {
                    return Err("message.duplicate_acp_options");
                }
            }
            let detail = serde_json::to_string_pretty(&input["toolCall"])
                .map_err(|_| "message.invalid_acp_request")?;
            if detail.len() > 48 * 1024 {
                return Err("message.acp_request_is_too_long");
            }
            let can_allow = self.edit
                && title.is_some()
                && options.iter().any(|option| option["kind"] == "allow_once");
            let title_key = title.is_none().then(|| "message.approval_requested".into());
            self.ask(
                id,
                method,
                input,
                AgentInteraction {
                    id: String::new(),
                    title: title.unwrap_or_default(),
                    title_key,
                    detail,
                    secret_warning: false,
                    can_allow,
                    questions: Vec::new(),
                },
                true,
            )?;
        } else if method == "session/update" {
            if self.phase != Phase::Running
                || event["params"]["sessionId"].as_str() != self.thread.as_deref()
            {
                return Err("message.acp_notification_does_not_belong_to_this_session");
            }
            let update = &event["params"]["update"];
            if update["sessionUpdate"] == "agent_message_chunk"
                && update["content"]["type"] == "text"
            {
                effects.preview = Some(self.preview(&update["content"]["text"])?);
            }
            if update["sessionUpdate"] == "usage_update" {
                let used = update["used"]
                    .as_u64()
                    .ok_or("message.invalid_acp_usage_information")?;
                let size = update["size"]
                    .as_u64()
                    .filter(|size| *size > 0 && *size >= used)
                    .ok_or("message.invalid_acp_usage_information")?;
                effects.usage = Some(json!({"contextUsedTokens":used,"contextWindowTokens":size}));
            }
        } else if method.is_empty() && event.get("id").is_some() {
            if event.get("error").is_some_and(|error| !error.is_null()) {
                return Err("message.the_acp_agent_reported_an_error");
            }
            match event["id"].as_str().unwrap_or("") {
                "initialize" if self.phase == Phase::Initializing => {
                    if event["result"]["protocolVersion"] != 1 {
                        return Err("message.unsupported_acp_version");
                    }
                    self.phase = Phase::StartingThread;
                    effects.send.push(json!({"jsonrpc":"2.0","id":"session","method":"session/new","params":{"cwd":self.repo,"mcpServers":[]}}));
                }
                "session" if self.phase == Phase::StartingThread => {
                    let id = text(&event["result"]["sessionId"], 256)?;
                    self.thread = Some(id.clone());
                    self.acp_setup.options = event["result"]["configOptions"].clone();
                    self.acp_setup.modes = event["result"]["modes"].clone();
                    effects.send.push(self.acp_configure()?);
                }
                "config-option" if self.phase == Phase::SettingOptions => {
                    let (id, value) = self
                        .acp_setup
                        .expected
                        .take()
                        .ok_or("message.acp_option_is_missing")?;
                    let options = &event["result"]["configOptions"];
                    if !options.as_array().is_some_and(|options| {
                        options
                            .iter()
                            .any(|option| option["id"] == id && option["currentValue"] == value)
                    }) {
                        return Err("message.the_acp_setting_was_not_confirmed");
                    }
                    self.acp_setup.options = options.clone();
                    effects.send.push(self.acp_configure()?);
                }
                "config-mode" if self.phase == Phase::SettingMode => {
                    let expected = self
                        .items
                        .remove("acp-mode")
                        .ok_or("message.acp_mode_is_missing")?;
                    let confirmed =
                        event["result"]["configOptions"]
                            .as_array()
                            .is_some_and(|options| {
                                options.iter().any(|option| {
                                    option["id"] == expected["id"]
                                        && option["currentValue"] == expected["value"]
                                })
                            });
                    if !confirmed {
                        return Err("message.the_acp_analysis_mode_was_not_confirmed");
                    }
                    let id = self
                        .thread
                        .clone()
                        .ok_or("message.acp_session_is_missing")?;
                    effects.send.push(self.acp_prompt(&id));
                }
                "mode" if self.phase == Phase::SettingMode => {
                    let id = self
                        .thread
                        .clone()
                        .ok_or("message.acp_session_is_missing")?;
                    effects.send.push(self.acp_prompt(&id));
                }
                "prompt" if self.phase == Phase::Running => {
                    if !self.pending.is_empty() {
                        return Err(
                            "message.acp_completion_arrived_while_requests_were_still_pending",
                        );
                    }
                    if !self.preview.is_empty() {
                        let message = self.redact(&self.preview);
                        let mut start = message.len().saturating_sub(64 * 1024);
                        while !message.is_char_boundary(start) {
                            start += 1;
                        }
                        let message = message[start..].to_owned();
                        effects.text.push(message.clone());
                        effects.summary = Some(message);
                    }
                    effects.complete = Some(event["result"]["stopReason"] == "end_turn");
                }
                _ => return Err("message.unexpected_acp_response"),
            }
        }
        Ok(effects)
    }

    fn acp_prompt(&mut self, id: &str) -> Value {
        self.phase = Phase::Running;
        json!({"jsonrpc":"2.0","id":"prompt","method":"session/prompt","params":{"sessionId":id,"prompt":[{"type":"text","text":self.prompt}]}})
    }

    fn acp_configure(&mut self) -> Result<Value> {
        let session = self
            .thread
            .clone()
            .ok_or("message.acp_session_is_missing")?;
        while self.acp_setup.stage < 2 {
            let (category, desired) = if self.acp_setup.stage == 0 {
                ("model", self.model.clone())
            } else {
                ("thought_level", self.effort.clone())
            };
            self.acp_setup.stage += 1;
            if desired.is_empty() {
                continue;
            }
            let option = self
                .acp_setup
                .options
                .as_array()
                .and_then(|options| {
                    options
                        .iter()
                        .find(|option| option["category"] == category && option["id"] == category)
                        .or_else(|| options.iter().find(|option| option["category"] == category))
                })
                .ok_or("message.this_setting_is_not_offered_by_the_acp_agent")?;
            let id = text(&option["id"], 128)?;
            let allowed = choice(option, |value| value == desired).is_some();
            if !allowed {
                return Err("message.this_setting_is_not_offered_by_the_acp_agent");
            }
            if option["currentValue"] == desired {
                continue;
            }
            self.acp_setup.expected = Some((id.clone(), desired.clone()));
            self.phase = Phase::SettingOptions;
            return Ok(
                json!({"jsonrpc":"2.0","id":"config-option","method":"session/set_config_option","params":{"sessionId":session,"configId":id,"value":desired}}),
            );
        }
        if self.edit {
            return Ok(self.acp_prompt(&session));
        }
        let mode_option = self
            .acp_setup
            .options
            .as_array()
            .and_then(|options| options.iter().find(|option| option["category"] == "mode"));
        if let Some(option) = mode_option {
            let config = text(&option["id"], 128)?;
            let value = choice(option, |value| value == "plan" || value == "read-only")
                .ok_or("message.this_agent_does_not_offer_a_verifiable_analysis_mode")?;
            self.items
                .insert("acp-mode".into(), json!({"id":config,"value":value}));
            self.phase = Phase::SettingMode;
            return Ok(
                json!({"jsonrpc":"2.0","id":"config-mode","method":"session/set_config_option","params":{"sessionId":session,"configId":config,"value":value}}),
            );
        }
        let mode = self.acp_setup.modes["availableModes"]
            .as_array()
            .and_then(|modes| {
                modes.iter().find_map(|mode| {
                    mode["id"]
                        .as_str()
                        .filter(|id| *id == "plan" || *id == "read-only")
                })
            })
            .ok_or("message.this_agent_does_not_offer_a_verifiable_analysis_mode")?;
        self.phase = Phase::SettingMode;
        Ok(
            json!({"jsonrpc":"2.0","id":"mode","method":"session/set_mode","params":{"sessionId":session,"modeId":mode}}),
        )
    }
}

pub(super) fn reply(pending: Pending, deny: bool) -> Result<Value> {
    let options = pending.input["options"]
        .as_array()
        .ok_or("message.invalid_acp_options")?;
    let desired = if deny { "reject_once" } else { "allow_once" };
    let outcome = match options.iter().find(|option| option["kind"] == desired) {
        Some(option) => json!({"outcome":"selected","optionId":text(&option["optionId"], 128)?}),
        None if deny => json!({"outcome":"cancelled"}),
        None => return Err("message.acp_permission_is_unavailable"),
    };
    Ok(json!({"jsonrpc":"2.0","id":pending.wire_id,"result":{"outcome":outcome}}))
}
