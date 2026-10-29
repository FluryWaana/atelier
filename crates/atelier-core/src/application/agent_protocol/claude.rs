use super::*;

pub(super) fn initialize() -> Value {
    json!({"type":"control_request","request_id":"initialize","request":{"subtype":"initialize"}})
}

pub(super) fn reply(pending: Pending, answer: &AgentAnswer, deny: bool) -> Result<Value> {
    let response = if deny {
        json!({"behavior":"deny","message":"Demande refusée par l’utilisateur."})
    } else {
        let mut input = pending.input;
        if !pending.view.questions.is_empty() {
            let questions = input["questions"]
                .as_array()
                .ok_or("message.questions_invalid")?;
            let mut answers = serde_json::Map::new();
            for (index, question) in questions.iter().enumerate() {
                answers.insert(
                    text(&question["question"], 4000)?,
                    json!(answer.answers[&index.to_string()]),
                );
            }
            input["answers"] = json!(answers);
        }
        json!({"behavior":"allow","updatedInput":input})
    };
    Ok(
        json!({"type":"control_response","response":{"subtype":"success","request_id":pending.wire_id,"response":response}}),
    )
}

pub(super) fn launch_arguments(edit: bool, model: &str, effort: &str) -> Vec<String> {
    let mut args = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        if edit {
            "default".into()
        } else {
            "plan".into()
        },
    ];
    if !model.is_empty() {
        args.extend(["--model".into(), model.into()]);
    }
    if !effort.is_empty() {
        args.extend(["--effort".into(), effort.into()]);
    }
    args
}

impl Protocol {
    pub(super) fn claude(&mut self, event: Value) -> Result<Effects> {
        let mut out = Effects::default();
        match event["type"].as_str().unwrap_or("") {
            "control_response" if event["response"]["request_id"] == "initialize" => {
                if self.phase != Phase::Initializing || event["response"]["subtype"] != "success" {
                    return Err("message.claude_initialization_failed");
                }
                self.phase = Phase::Running;
                out.send.push(json!({"type":"user","session_id":"","parent_tool_use_id":null,"message":{"role":"user","content":self.prompt}}));
            }
            "control_cancel_request" => {
                self.pending
                    .retain(|_, pending| pending.wire_id != event["request_id"]);
            }
            "control_request" => {
                if self.phase != Phase::Running {
                    return Err("message.request_before_agent_start");
                }
                let id = wire_id(&event["request_id"])?;
                let request = &event["request"];
                if request["subtype"] != "can_use_tool" {
                    out.send.push(json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"Interaction non prise en charge par Atelier."}}));
                    return Ok(out);
                }
                let tool = text(&request["tool_name"], 128)?;
                let input = &request["input"];
                if !input.is_object() {
                    return Err("message.tool_parameters_invalid");
                }
                let detail =
                    serde_json::to_string_pretty(input).map_err(|_| "message.request_invalid")?;
                if detail.len() > 48 * 1024 {
                    return Err("message.agent_request_too_long");
                }
                let questions = if tool == "AskUserQuestion" {
                    self.questions(&input["questions"])?
                } else {
                    Vec::new()
                };
                let allow = self.edit
                    || !["Write", "Edit", "MultiEdit", "NotebookEdit", "ExitPlanMode"]
                        .contains(&tool.as_str());
                self.ask(
                    id,
                    &tool,
                    input.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: tool.clone(),
                        title_key: None,
                        detail: if questions.is_empty() {
                            detail
                        } else {
                            String::new()
                        },
                        secret_warning: false,
                        can_allow: allow,
                        questions,
                    },
                    true,
                )?;
            }
            "assistant" => {
                if let Some(content) = event["message"]["content"].as_array() {
                    for item in content {
                        if item["type"] == "text" {
                            out.text.push(text(&item["text"], 64 * 1024)?);
                        } else if item["type"] == "tool_use" {
                            out.text.push(format!("→ {}", text(&item["name"], 128)?));
                        }
                    }
                }
            }
            "stream_event" => {
                if event["event"]["type"] == "content_block_start"
                    && event["event"]["content_block"]["type"] == "text"
                {
                    self.preview.clear();
                }
                if event["event"]["delta"]["type"] == "text_delta" {
                    out.preview = Some(self.preview(&event["event"]["delta"]["text"])?);
                }
            }
            "result" => {
                if self.phase != Phase::Running {
                    return Err("message.claude_result_unexpected");
                }
                if let Some(message) = event["result"]
                    .as_str()
                    .filter(|message| !message.is_empty())
                {
                    out.summary = Some(text(&json!(message), 64 * 1024)?);
                }
                out.usage = usage(
                    &event["usage"],
                    &["input_tokens", "cache_read_input_tokens", "output_tokens"],
                );
                if let Some(amount) = event["total_cost_usd"].as_f64().filter(|amount| {
                    amount.is_finite()
                        && (0.0..=1_000_000.0).contains(amount)
                        && !(*amount == 0.0 && event["subtype"] == "error_during_execution")
                }) {
                    let usage = out.usage.get_or_insert_with(|| json!({}));
                    usage["estimatedCostUsd"] = json!(amount);
                    usage["costSource"] = json!("claude-cli");
                    usage["costScope"] = json!("cli-session");
                }
                out.complete = Some(
                    event["is_error"] == false
                        && event["subtype"] == "success"
                        && event.get("permission_denials").is_none_or(|value| {
                            value.is_null() || value.as_array().is_some_and(Vec::is_empty)
                        }),
                );
            }
            _ => {}
        }
        Ok(out)
    }
}
