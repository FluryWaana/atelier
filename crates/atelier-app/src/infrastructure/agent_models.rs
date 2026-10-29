use super::{
    agent_account::reflected, agent_diagnostics::Environment, process_tree::ContainedChild,
};
use atelier_core::{contracts::AgentModel, domain::agent_catalog::ProtocolKind};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn safe(value: &Value, limit: usize) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

fn model_id(value: &Value) -> Option<String> {
    safe(value, 180).filter(|id| {
        id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-[]".contains(&byte))
    })
}

fn effort_id(value: &Value) -> Option<String> {
    safe(value, 40).filter(|id| {
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    })
}

fn parse(value: &Value, kind: ProtocolKind) -> Vec<AgentModel> {
    if kind == ProtocolKind::Claude {
        return value["models"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .filter_map(|item| {
                let id = model_id(&item["value"])?;
                let name = safe(&item["displayName"], 200).unwrap_or_else(|| id.clone());
                let efforts = item["supportedEffortLevels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(20)
                    .filter_map(effort_id)
                    .collect();
                let is_default = id == "default";
                Some(AgentModel {
                    id,
                    name,
                    efforts,
                    is_default,
                })
            })
            .collect();
    }
    if kind == ProtocolKind::Codex {
        return value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .filter_map(|item| {
                let id = model_id(&item["model"])?;
                let name = safe(&item["displayName"], 200).unwrap_or_else(|| id.clone());
                let efforts = item["supportedReasoningEfforts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(20)
                    .filter_map(|entry| effort_id(&entry["reasoningEffort"]))
                    .collect();
                Some(AgentModel {
                    id,
                    name,
                    efforts,
                    is_default: item["isDefault"] == true,
                })
            })
            .collect();
    }
    let options = value["configOptions"].as_array();
    let model = options.and_then(|items| {
        items
            .iter()
            .find(|item| item["category"] == "model" && item["id"] == "model")
            .or_else(|| items.iter().find(|item| item["category"] == "model"))
    });
    let Some(model) = model else {
        return Vec::new();
    };
    let effort = options.and_then(|items| {
        items
            .iter()
            .find(|item| item["category"] == "thought_level")
    });
    let mut efforts = Vec::new();
    if let Some(effort) = effort {
        for item in effort["options"].as_array().into_iter().flatten().take(20) {
            if let Some(group) = item["options"].as_array() {
                efforts.extend(
                    group
                        .iter()
                        .take(20)
                        .filter_map(|item| effort_id(&item["value"])),
                );
            } else if let Some(value) = effort_id(&item["value"]) {
                efforts.push(value);
            }
        }
        efforts.truncate(20);
    }
    let mut values = Vec::new();
    for item in model["options"].as_array().into_iter().flatten().take(200) {
        if item["options"].is_array() {
            values.extend(item["options"].as_array().into_iter().flatten().take(200));
        } else {
            values.push(item);
        }
    }
    values
        .into_iter()
        .take(200)
        .filter_map(|item| {
            let id = safe(&item["value"], 180)?;
            let name = safe(&item["name"], 200).unwrap_or_else(|| id.clone());
            let is_default = model["currentValue"].as_str() == Some(&id);
            let available = if is_default {
                efforts.clone()
            } else {
                Vec::new()
            };
            Some(AgentModel {
                id,
                name,
                efforts: available,
                is_default,
            })
        })
        .collect()
}

pub(super) async fn read(
    binary: &Path,
    environment: &Environment,
    kind: ProtocolKind,
    args: &[String],
    probe_model: &str,
) -> crate::Result<Vec<AgentModel>> {
    if !matches!(
        kind,
        ProtocolKind::Codex | ProtocolKind::Claude | ProtocolKind::Acp
    ) {
        return Ok(Vec::new());
    }
    let directory = tempfile::tempdir()?;
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .current_dir(directory.path())
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let secrets = environment
        .iter()
        .filter(|(key, _)| key.contains("KEY") || key.contains("TOKEN"))
        .filter_map(|(_, value)| {
            value
                .to_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    let mut child = ContainedChild::spawn(&mut command).await?;
    let mut input = child
        .child
        .stdin
        .take()
        .ok_or("message.invalid_server_response")?;
    let output = child
        .child
        .stdout
        .take()
        .ok_or("message.invalid_server_response")?;
    let work = async {
        let initial = if kind == ProtocolKind::Claude {
            json!({"type":"control_request","request_id":"initialize","request":{"subtype":"initialize"}})
        } else if kind == ProtocolKind::Codex {
            json!({"id":"initialize","method":"initialize","params":{"clientInfo":{"name":"atelier_kanban","version":env!("CARGO_PKG_VERSION")}}})
        } else {
            json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"atelier-kanban","version":env!("CARGO_PKG_VERSION")}}})
        };
        input.write_all(format!("{initial}\n").as_bytes()).await?;
        let mut reader = BufReader::new(output.take(1024 * 1024));
        let mut models = Vec::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut selected = None;
        for _ in 0..64 {
            let mut line = Vec::new();
            if reader.read_until(b'\n', &mut line).await? == 0 || !line.ends_with(b"\n") {
                return Err("message.invalid_server_response".into());
            }
            let value: Value = serde_json::from_slice(&line)?;
            if reflected(&value, &secrets) {
                return Err("message.invalid_server_response".into());
            }
            if kind == ProtocolKind::Claude
                && value["type"] == "control_response"
                && value["response"]["request_id"] == "initialize"
            {
                if value["response"]["subtype"] != "success" {
                    return Err("message.invalid_server_response".into());
                }
                return Ok(parse(&value["response"]["response"], kind));
            } else if value["id"] == "initialize" {
                if !value["error"].is_null() {
                    return Err("message.invalid_server_response".into());
                }
                let request = if kind == ProtocolKind::Codex {
                    input.write_all(b"{\"method\":\"initialized\"}\n").await?;
                    json!({"id":"models","method":"model/list","params":{"limit":200}})
                } else {
                    json!({"jsonrpc":"2.0","id":"models","method":"session/new","params":{"cwd":directory.path().to_string_lossy(),"mcpServers":[]}})
                };
                input.write_all(format!("{request}\n").as_bytes()).await?;
            } else if value["id"] == "models" {
                if !value["error"].is_null() {
                    return Err("message.invalid_server_response".into());
                }
                if kind == ProtocolKind::Acp && !probe_model.is_empty() {
                    let result = &value["result"];
                    let choices = parse(result, kind);
                    let model_option = result["configOptions"].as_array().and_then(|options| {
                        options
                            .iter()
                            .find(|option| option["category"] == "model" && option["id"] == "model")
                            .or_else(|| options.iter().find(|option| option["category"] == "model"))
                    });
                    if choices
                        .iter()
                        .any(|choice| choice.id == probe_model && !choice.is_default)
                        && let Some(config_id) =
                            model_option.and_then(|option| safe(&option["id"], 128))
                        && let Some(session_id) = safe(&result["sessionId"], 256)
                    {
                        selected = Some((config_id.clone(), probe_model.to_owned()));
                        let request = json!({"jsonrpc":"2.0","id":"select-model","method":"session/set_config_option","params":{"sessionId":session_id,"configId":config_id,"value":probe_model}});
                        input.write_all(format!("{request}\n").as_bytes()).await?;
                        continue;
                    }
                }
                models.extend(parse(&value["result"], kind));
                if kind == ProtocolKind::Codex && !value["result"]["nextCursor"].is_null() {
                    let cursor = safe(&value["result"]["nextCursor"], 512)
                        .ok_or("message.invalid_server_response")?;
                    if cursors.len() >= 5 || !cursors.insert(cursor.clone()) {
                        return Err("message.invalid_server_response".into());
                    }
                    let request = json!({"id":"models","method":"model/list","params":{"limit":200,"cursor":cursor}});
                    input.write_all(format!("{request}\n").as_bytes()).await?;
                } else {
                    return Ok(models);
                }
            } else if value["id"] == "select-model" {
                let (config_id, expected) =
                    selected.take().ok_or("message.invalid_server_response")?;
                if !value["error"].is_null()
                    || !value["result"]["configOptions"]
                        .as_array()
                        .is_some_and(|options| {
                            options.iter().any(|option| {
                                option["id"] == config_id && option["currentValue"] == expected
                            })
                        })
                {
                    return Err("message.invalid_server_response".into());
                }
                return Ok(parse(&value["result"], kind));
            }
        }
        Err("message.invalid_server_response".into())
    };
    let result = tokio::time::timeout(Duration::from_secs(10), work)
        .await
        .unwrap_or_else(|_| Err("message.invalid_server_response".into()));
    child.stop().await?;
    result
}

#[cfg(test)]
#[test]
fn parses_bounded_model_and_effort_choices() {
    let choices = parse(
        &json!({"data":[{"model":"synthetic","displayName":"Synthetic","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]}),
        ProtocolKind::Codex,
    );
    assert_eq!(choices[0].efforts, ["high"]);
    let choices = parse(
        &json!({"models":[{"value":"opus[1m]","displayName":"Opus","supportedEffortLevels":["low","max"]}]}),
        ProtocolKind::Claude,
    );
    assert_eq!(choices[0].id, "opus[1m]");
    assert_eq!(choices[0].efforts, ["low", "max"]);
    let choices = parse(
        &json!({"configOptions":[{"category":"model","currentValue":"fake","options":[{"options":[{"value":"fake","name":"Fake"},{"value":"{\"model\":\"custom\"}"}]}]},{"category":"thought_level","options":[{"value":"high"}]}]}),
        ProtocolKind::Acp,
    );
    assert_eq!(choices[0].id, "fake");
    assert_eq!(choices.len(), 2);
    assert!(choices[0].is_default);
    assert_eq!(choices[0].efforts, ["high"]);
    let choices = parse(
        &json!({"configOptions":[{"id":"provider","category":"model","options":[{"value":"provider-name"}]},{"id":"model","category":"model","options":[{"value":"actual-model"}]}]}),
        ProtocolKind::Acp,
    );
    assert_eq!(choices[0].id, "actual-model");
}
