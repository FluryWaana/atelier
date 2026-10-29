use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub name: String,
    pub kind: String,
    pub remaining_tenths: u16,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Available,
    Unavailable,
    Api,
    Disconnected,
    Unsupported,
    RateLimited,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub state: State,
    pub account: Option<String>,
    pub subscription: Option<String>,
    pub windows: Vec<Window>,
    pub observed_at: String,
    #[serde(default)]
    pub retry_at: Option<String>,
}
impl Report {
    pub fn unavailable(state: State) -> Self {
        Self {
            state,
            account: None,
            subscription: None,
            windows: Vec::new(),
            observed_at: String::new(),
            retry_at: None,
        }
    }
}
fn text(value: &Value, max: usize) -> Result<Option<String>, &'static str> {
    if value.is_null() {
        return Ok(None);
    }
    let text = value
        .as_str()
        .filter(|text| text.encode_utf16().count() <= max && !text.chars().any(char::is_control))
        .ok_or("message.account_metadata_invalid")?;
    Ok((!text.trim().is_empty()).then(|| text.into()))
}
pub fn windows(value: &Value) -> Result<Vec<Window>, &'static str> {
    let buckets = match value.get("rateLimitsByLimitId") {
        Some(Value::Object(values)) if values.len() <= 20 => values.values().collect::<Vec<_>>(),
        Some(Value::Null) | None => vec![&value["rateLimits"]],
        _ => return Err("message.quota_report_invalid"),
    };
    let mut windows = Vec::new();
    for bucket in buckets {
        if bucket.is_null() {
            continue;
        }
        if !bucket.is_object() {
            return Err("message.quota_report_invalid");
        }
        let name = text(&bucket["limitName"], 100)?
            .or(text(&bucket["limitId"], 100)?)
            .unwrap_or_else(|| "Codex".into());
        for kind in ["primary", "secondary"] {
            let window = &bucket[kind];
            if window.is_null() {
                continue;
            }
            let used = window["usedPercent"]
                .as_f64()
                .filter(|used| used.is_finite() && (0.0..=100.0).contains(used))
                .ok_or("message.quota_percentage_invalid")?;
            let minutes = if window["windowDurationMins"].is_null() {
                None
            } else {
                Some(
                    window["windowDurationMins"]
                        .as_u64()
                        .filter(|value| *value > 0 && *value <= 525_600)
                        .ok_or("message.quota_duration_invalid")? as u32,
                )
            };
            let resets_at = if window["resetsAt"].is_null() {
                None
            } else {
                Some(
                    window["resetsAt"]
                        .as_i64()
                        .filter(|value| *value > 0 && *value < 253_402_300_800)
                        .ok_or("message.quota_date_invalid")?,
                )
            };
            windows.push(Window {
                name: name.clone(),
                kind: kind.into(),
                remaining_tenths: ((100.0 - used) * 10.0).round() as u16,
                window_minutes: minutes,
                resets_at,
            });
        }
    }
    Ok(windows)
}
pub struct Probe {
    phase: Phase,
    report: Report,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initialize,
    Account,
    Quotas,
    Done,
}
impl Default for Probe {
    fn default() -> Self {
        Self {
            phase: Phase::Initialize,
            report: Report::unavailable(State::Unavailable),
        }
    }
}
impl Probe {
    pub fn start(&self) -> Value {
        json!({"id":"account-initialize","method":"initialize","params":{"clientInfo":{"name":"atelier_kanban","title":"Atelier account read","version":env!("CARGO_PKG_VERSION")}}})
    }
    pub fn receive(&mut self, value: Value) -> Result<(Vec<Value>, Option<Report>), &'static str> {
        if self.phase == Phase::Done {
            return Err("message.account_read_already_finished");
        }
        if !value.is_object() {
            return Err("message.account_response_invalid");
        }
        if value.get("method").is_some() {
            return Ok((value.get("id").map(|id|json!({"id":id,"error":{"code":-32601,"message":"Read-only account probe"}})).into_iter().collect(),None));
        }
        let id = match self.phase {
            Phase::Initialize => "account-initialize",
            Phase::Account => "account-read",
            _ => "account-quotas",
        };
        if value["id"] != id {
            return Ok((Vec::new(), None));
        }
        if value.get("error").is_some() || !value["result"].is_object() {
            return Err("message.account_read_refused");
        }
        match self.phase {
            Phase::Initialize => {
                self.phase = Phase::Account;
                Ok((
                    vec![
                        json!({"method":"initialized","params":{}}),
                        json!({"id":"account-read","method":"account/read","params":{"refreshToken":false}}),
                    ],
                    None,
                ))
            }
            Phase::Account => {
                let account = &value["result"]["account"];
                if account["type"] != "chatgpt" {
                    self.report.state = if account.is_null() {
                        State::Disconnected
                    } else if account["type"] == "apiKey" {
                        State::Api
                    } else {
                        State::Unsupported
                    };
                    self.phase = Phase::Done;
                    return Ok((Vec::new(), Some(self.report.clone())));
                }
                self.report.account = text(&account["email"], 200)?;
                self.report.subscription = text(&account["planType"], 80)?;
                self.phase = Phase::Quotas;
                Ok((
                    vec![json!({"id":"account-quotas","method":"account/rateLimits/read"})],
                    None,
                ))
            }
            _ => {
                self.report.windows = windows(&value["result"])?;
                self.report.state = if self.report.windows.is_empty() {
                    State::Unavailable
                } else {
                    State::Available
                };
                self.phase = Phase::Done;
                Ok((Vec::new(), Some(self.report.clone())))
            }
        }
    }
}
