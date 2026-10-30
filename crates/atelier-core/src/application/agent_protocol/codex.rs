use super::*;

pub(super) fn initialize() -> Value {
    json!({"id":"initialize","method":"initialize","params":{"clientInfo":{"name":"atelier_kanban","title":"Atelier Kanban","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}})
}

pub(super) fn reply(pending: Pending, answer: &AgentAnswer, deny: bool) -> Result<Value> {
    let response = if pending.method == "item/tool/requestUserInput" {
        let mut answers = serde_json::Map::new();
        for (index, question) in pending.input["questions"]
            .as_array()
            .ok_or("message.questions_invalid")?
            .iter()
            .enumerate()
        {
            answers.insert(text(&question["id"],128)?,json!({"answers":if deny{Vec::new()}else{answer.answers[&index.to_string()].clone()}}));
        }
        json!({"answers":answers})
    } else if pending.method == "item/permissions/requestApproval" {
        json!({"permissions":if deny{json!({})}else{pending.input["permissions"].clone()},"scope":"turn"})
    } else {
        json!({"decision":if deny { if pending.input.get("availableDecisions").is_none_or(|values| values.is_null() || values.as_array().is_some_and(|values| values.iter().any(|value| value == "decline"))){"decline"}else{"cancel"} } else {"accept"}})
    };
    Ok(json!({"id":pending.wire_id,"result":response}))
}

pub(super) fn launch_arguments(api_key: bool) -> Vec<String> {
    let mut args = vec!["app-server".into()];
    if api_key {
        args.extend([
            "-c".into(),
            "cli_auth_credentials_store=\"ephemeral\"".into(),
        ]);
    }
    args
}

impl Protocol {
    pub(super) fn codex(&mut self, event: Value) -> Result<Effects> {
        let mut out = Effects::default();
        let p = &event["params"];
        let method = event["method"].as_str().unwrap_or("");
        if p.get("threadId").is_some()
            && self
                .thread
                .as_ref()
                .is_some_and(|thread| p["threadId"] != *thread)
        {
            return Ok(out);
        }
        if !method.is_empty() && event.get("id").is_some() {
            if self.phase != Phase::Running {
                return Err("message.request_before_agent_start");
            }
            let id = wire_id(&event["id"])?;
            let detail = serde_json::to_string_pretty(p).map_err(|_| "message.request_invalid")?;
            if detail.len() > 48 * 1024 {
                return Err("message.agent_request_too_long");
            }
            if method == "item/tool/requestUserInput" {
                let questions = self.questions(&p["questions"])?;
                let original = p["questions"]
                    .as_array()
                    .ok_or("message.questions_invalid")?;
                let mut unique = std::collections::BTreeSet::new();
                for question in original {
                    if !unique.insert(text(&question["id"], 128)?) {
                        return Err("message.question_id_duplicate");
                    }
                }
                self.ask(
                    id,
                    method,
                    p.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: String::new(),
                        title_key: Some("message.the_agent_needs_your_reply".into()),
                        detail: String::new(),
                        secret_warning: false,
                        can_allow: true,
                        questions,
                    },
                    p["isBlocking"] != false,
                )?;
            } else if [
                "item/commandExecution/requestApproval",
                "item/fileChange/requestApproval",
                "item/permissions/requestApproval",
            ]
            .contains(&method)
            {
                let item = p["itemId"].as_str().and_then(|id| self.items.get(id));
                let allow = match method {
                    "item/fileChange/requestApproval" => {
                        self.edit
                            && item
                                .and_then(|item| item["changes"].as_array())
                                .is_some_and(|changes| {
                                    !changes.is_empty()
                                        && changes.iter().all(|change| {
                                            change["path"]
                                                .as_str()
                                                .is_some_and(|path| !path.is_empty())
                                                && change["diff"].is_string()
                                        })
                                })
                    }
                    "item/permissions/requestApproval" => self.edit && p["permissions"].is_object(),
                    _ => p["command"].is_string(),
                } && p.get("availableDecisions").is_none_or(|values| {
                    values.is_null()
                        || values
                            .as_array()
                            .is_some_and(|values| values.iter().any(|value| value == "accept"))
                });
                let detail = serde_json::to_string_pretty(&json!({"request":p,"changes":item}))
                    .map_err(|_| "message.request_invalid")?;
                if detail.len() > 64 * 1024 {
                    return Err("message.agent_request_too_long");
                }
                self.ask(
                    id,
                    method,
                    p.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: String::new(),
                        title_key: Some("message.approval_requested".into()),
                        detail,
                        secret_warning: false,
                        can_allow: allow,
                        questions: Vec::new(),
                    },
                    true,
                )?;
            } else {
                out.send.push(json!({"id":id,"error":{"code":-32601,"message":"Interaction non prise en charge par Atelier."}}));
                out.text.push("Interaction inconnue refusée.".into());
            }
            return Ok(out);
        }
        if method == "serverRequest/resolved" {
            self.pending
                .retain(|_, pending| pending.wire_id != p["requestId"]);
        } else if event.get("id").is_some() {
            if event.get("error").is_some_and(|value| !value.is_null()) {
                return Err("message.codex_protocol_error");
            }
            match event["id"].as_str().unwrap_or("") {
                "initialize" if self.phase == Phase::Initializing => {
                    out.send.push(json!({"method":"initialized"}));
                    if self.api_key.is_empty() {
                        out.send.push(self.thread());
                    } else {
                        self.phase = Phase::CheckingCredentials;
                        out.send.push(json!({"id":"credential-storage","method":"config/read","params":{"includeLayers":false}}));
                    }
                }
                "credential-storage" if self.phase == Phase::CheckingCredentials => {
                    if event["result"]["config"]["cli_auth_credentials_store"] != "ephemeral" {
                        return Err("message.codex_memory_credentials_required");
                    }
                    self.phase = Phase::Authenticating;
                    out.send.push(json!({"id":"login","method":"account/login/start","params":{"type":"apiKey","apiKey":self.api_key}}));
                }
                "login" if self.phase == Phase::Authenticating => out.send.push(self.thread()),
                "thread" if self.phase == Phase::StartingThread => {
                    let id = text(&event["result"]["thread"]["id"], 256)?;
                    out.send.push(json!({"id":"turn","method":"turn/start","params":{"threadId":id,"input":[{"type":"text","text":self.prompt}],"effort":if self.effort.is_empty(){Value::Null}else{json!(self.effort)}}}));
                    self.thread = Some(id);
                    self.phase = Phase::Running;
                }
                "turn" if self.phase == Phase::Running => {}
                _ => return Err("message.protocol_response_unexpected"),
            }
        } else if method == "item/agentMessage/delta" {
            out.preview = Some(self.preview(&p["delta"])?);
        } else if method == "item/started" && p["item"]["type"] == "agentMessage" {
            self.preview.clear();
            out.preview = Some(String::new());
        } else if method == "item/started" && p["item"]["type"] == "commandExecution" {
            if let Some(command) = p["item"]["command"].as_str() {
                out.text.push(format!("Commande : {command}"));
            }
        } else if method == "item/started" && p["item"]["type"] == "fileChange" {
            if self.items.len() >= 32 {
                return Err("message.too_many_simultaneous_changes");
            }
            self.items
                .insert(text(&p["item"]["id"], 128)?, p["item"].clone());
        } else if method == "item/completed" {
            if p["item"]["type"] == "commandExecution"
                && p["item"]["aggregatedOutput"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            {
                out.text
                    .push(text(&p["item"]["aggregatedOutput"], 64 * 1024)?);
            }
            if p["item"]["type"] == "agentMessage" {
                let message = text(&p["item"]["text"], 64 * 1024)?;
                out.text.push(message.clone());
                out.summary = Some(message);
                self.preview.clear();
                out.preview = Some(String::new());
            }
            if let Some(id) = p["item"]["id"].as_str() {
                self.items.remove(id);
            }
        } else if method == "thread/tokenUsage/updated" {
            if !self
                .thread
                .as_ref()
                .is_some_and(|thread| p["threadId"].as_str() == Some(thread.as_str()))
            {
                return Ok(out);
            }
            out.usage = usage(
                &p["tokenUsage"]["total"],
                &["inputTokens", "cachedInputTokens", "outputTokens"],
            );
            if self.cost_eligible
                && let Some(usage) = &mut out.usage
                && let Some(amount) = codex_cost(&self.model, usage)
            {
                usage["estimatedCostUsd"] = json!(amount);
                usage["costSource"] = json!("openai-standard-api");
                usage["costScope"] = json!("requested-model-thread");
                usage["pricingModel"] = json!(self.model);
                usage["pricingDate"] = json!("2026-10-26");
            }
            self.codex_usage = out.usage.clone();
        } else if method == "model/rerouted"
            && self
                .thread
                .as_ref()
                .is_some_and(|thread| p["threadId"].as_str() == Some(thread.as_str()))
        {
            self.cost_eligible = false;
            if let Some(mut usage) = self.codex_usage.clone() {
                if let Some(fields) = usage.as_object_mut() {
                    for key in [
                        "estimatedCostUsd",
                        "costSource",
                        "costScope",
                        "pricingModel",
                        "pricingDate",
                    ] {
                        fields.remove(key);
                    }
                }
                self.codex_usage = Some(usage.clone());
                out.usage = Some(usage);
            }
        } else if method == "error" && p["willRetry"] != true {
            self.failed = true;
            out.text.push("Erreur signalée par Codex.".into());
        } else if method == "turn/completed" {
            if self.phase != Phase::Running {
                return Err("message.turn_end_unexpected");
            }
            out.complete = Some(p["turn"]["status"] == "completed");
        }
        Ok(out)
    }
}

fn codex_cost(model: &str, usage: &Value) -> Option<f64> {
    // shortcut: dated standard short-context API equivalent, excluding tier/context/region/tool charges; revisit when CLI reports them.
    // Official snapshot: https://developers.openai.com/api/docs/pricing (2026-10-26), milli-USD per million tokens.
    let [input_price, cached_price, output_price]: [u128; 3] = match model {
        "gpt-6.1-sol" => [2000, 100, 10000],
        "gpt-6.1-terra" => [1000, 50, 5000],
        "gpt-6-astra" => [10000, 1000, 50000],
        "gpt-6-luna" => [100, 10, 500],
        "gpt-5.6-sol" => [4000, 400, 20000],
        "gpt-5.3-codex" => [1750, 175, 14000],
        _ => return None,
    };
    let input = usage["inputTokens"].as_u64()?;
    let cached = usage["cachedInputTokens"].as_u64()?;
    let output = usage["outputTokens"].as_u64()?;
    let uncached = input.checked_sub(cached)?;
    let amount = (u128::from(uncached) * input_price
        + u128::from(cached) * cached_price
        + u128::from(output) * output_price) as f64
        / 1_000_000_000.0;
    (amount.is_finite() && amount <= 1_000_000.0).then_some(amount)
}
