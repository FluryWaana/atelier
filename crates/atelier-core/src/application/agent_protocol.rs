use crate::contracts::{AgentAnswer, AgentInteraction, AgentQuestion};
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Codex,
    Claude,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    CheckingCredentials,
    Authenticating,
    StartingThread,
    Running,
}

#[derive(Default)]
pub struct Effects {
    pub send: Vec<Value>,
    pub text: Vec<String>,
    pub complete: Option<bool>,
    pub summary: Option<String>,
    pub usage: Option<Value>,
    pub preview: Option<String>,
    pub interactions_changed: bool,
}

struct Pending {
    wire_id: Value,
    method: String,
    input: Value,
    view: AgentInteraction,
    blocking: bool,
}

pub struct Protocol {
    kind: Kind,
    edit: bool,
    repo: String,
    prompt: String,
    model: String,
    effort: String,
    api_key: String,
    phase: Phase,
    thread: Option<String>,
    pending: BTreeMap<String, Pending>,
    items: BTreeMap<String, Value>,
    serial: u64,
    secrets: Vec<String>,
    failed: bool,
    finished: bool,
    preview: String,
}

pub struct ProtocolConfig {
    pub kind: Kind,
    pub edit: bool,
    pub repo: String,
    pub prompt: String,
    pub model: String,
    pub effort: String,
    pub api_key: String,
    pub secrets: Vec<String>,
}

fn text(value: &Value, limit: usize) -> Result<String> {
    value
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= limit && !value.contains('\0'))
        .map(str::to_owned)
        .ok_or("Message de protocole invalide.")
}
fn wire_id(value: &Value) -> Result<Value> {
    if value.is_i64()
        || value.is_u64()
        || value
            .as_str()
            .is_some_and(|value| !value.is_empty() && value.len() <= 128)
    {
        Ok(value.clone())
    } else {
        Err("Identifiant de protocole invalide.")
    }
}

impl Protocol {
    pub fn new(config: ProtocolConfig) -> Self {
        let ProtocolConfig {
            kind,
            edit,
            repo,
            prompt,
            model,
            effort,
            api_key,
            secrets,
        } = config;
        let mut protocol = Self {
            kind,
            edit,
            repo,
            prompt,
            model,
            effort,
            api_key,
            phase: Phase::New,
            thread: None,
            pending: BTreeMap::new(),
            items: BTreeMap::new(),
            serial: 0,
            secrets: Vec::new(),
            failed: false,
            finished: false,
            preview: String::new(),
        };
        for secret in secrets
            .into_iter()
            .chain(std::iter::once(protocol.api_key.clone()))
        {
            protocol.protect(&secret);
        }
        protocol
    }
    pub fn protect(&mut self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        for part in std::iter::once(secret)
            .chain(secret.lines())
            .filter(|part| !part.is_empty())
        {
            let encoded = serde_json::to_string(part).expect("string serialization");
            for value in [part.to_owned(), encoded[1..encoded.len() - 1].to_owned()] {
                if !self.secrets.contains(&value) {
                    self.secrets.push(value);
                }
            }
        }
        self.secrets
            .sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    }
    pub fn redact(&self, value: &str) -> String {
        self.secrets.iter().fold(value.to_owned(), |value, secret| {
            value.replace(secret, "[secret]")
        })
    }
    pub fn interactions(&self) -> Vec<AgentInteraction> {
        self.pending
            .values()
            .map(|pending| {
                let mut view = pending.view.clone();
                view.title = self.redact(&view.title);
                view.detail = self.redact(&view.detail);
                for question in &mut view.questions {
                    question.question = self.redact(&question.question);
                    for option in &mut question.options {
                        *option = self.redact(option);
                    }
                }
                view
            })
            .collect()
    }
    pub fn waiting(&self) -> bool {
        self.pending.values().any(|pending| pending.blocking)
    }
    pub fn ready(&self) -> bool {
        self.phase == Phase::Running
    }
    fn preview(&mut self, delta: &Value) -> Result<String> {
        let delta = delta.as_str().ok_or("Fragment public invalide.")?;
        if self.preview.len().saturating_add(delta.len()) > 1024 * 1024 {
            return Err("Réponse de l’agent trop longue.");
        }
        self.preview.push_str(delta);
        // Mask the whole buffer before withholding the suffix that could contain an incomplete secret.
        let value = self.redact(&self.preview);
        let mut end = value
            .len()
            .saturating_sub(self.secrets.iter().map(String::len).max().unwrap_or(0));
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        let mut start = end.saturating_sub(4000);
        while !value.is_char_boundary(start) {
            start += 1;
        }
        Ok(value[start..end].into())
    }
    pub fn start(&mut self) -> Result<Effects> {
        if self.phase != Phase::New {
            return Err("Session déjà initialisée.");
        }
        self.phase = Phase::Initializing;
        Ok(Effects {
            send: vec![match self.kind {
                Kind::Codex => {
                    json!({"id":"initialize","method":"initialize","params":{"clientInfo":{"name":"atelier_kanban","title":"Atelier Kanban","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}})
                }
                Kind::Claude => {
                    json!({"type":"control_request","request_id":"initialize","request":{"subtype":"initialize"}})
                }
            }],
            ..Effects::default()
        })
    }
    fn thread(&mut self) -> Value {
        self.phase = Phase::StartingThread;
        json!({"id":"thread","method":"thread/start","params":{"cwd":self.repo,"model":if self.model.is_empty(){Value::Null}else{json!(self.model)},"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":if self.edit{"workspace-write"}else{"read-only"},"config":{"features.default_mode_request_user_input":true}}})
    }
    fn ask(
        &mut self,
        id: Value,
        method: &str,
        input: Value,
        mut view: AgentInteraction,
        blocking: bool,
    ) -> Result<()> {
        if self.pending.len() >= 32 || self.pending.values().any(|pending| pending.wire_id == id) {
            return Err("Demandes de l’agent trop nombreuses ou dupliquées.");
        }
        self.serial += 1;
        let key = format!("interaction-{}", self.serial);
        view.id = key.clone();
        self.pending.insert(
            key,
            Pending {
                wire_id: id,
                method: method.into(),
                input,
                view,
                blocking,
            },
        );
        Ok(())
    }
    fn questions(&self, value: &Value) -> Result<Vec<AgentQuestion>> {
        let list = value
            .as_array()
            .filter(|list| !list.is_empty() && list.len() <= 8)
            .ok_or("Questions de l’agent invalides.")?;
        let mut unique = std::collections::BTreeSet::new();
        for question in list {
            if !unique.insert(text(&question["question"], 4000)?) {
                return Err("Question dupliquée.");
            }
        }
        list.iter()
            .enumerate()
            .map(|(index, value)| {
                for field in ["multiSelect", "isSecret"] {
                    if value
                        .get(field)
                        .is_some_and(|value| !value.is_null() && !value.is_boolean())
                    {
                        return Err("Questions de l’agent invalides.");
                    }
                }
                if value
                    .get("options")
                    .is_some_and(|value| !value.is_null() && !value.is_array())
                {
                    return Err("Questions de l’agent invalides.");
                }
                let options = value.get("options").and_then(Value::as_array);
                if options.is_some_and(|options| options.len() > 20) {
                    return Err("Trop de choix proposés par l’agent.");
                }
                let options = options
                    .into_iter()
                    .flatten()
                    .map(|option| {
                        text(
                            if option.is_string() {
                                option
                            } else {
                                &option["label"]
                            },
                            1000,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                if options
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != options.len()
                {
                    return Err("Choix de question dupliqué.");
                }
                Ok(AgentQuestion {
                    id: index.to_string(),
                    question: text(&value["question"], 4000)?,
                    options,
                    multi_select: value["multiSelect"] == true,
                    is_secret: value["isSecret"] == true,
                })
            })
            .collect()
    }
    pub fn receive(&mut self, event: Value) -> Result<Effects> {
        if self.finished {
            return Ok(Effects::default());
        }
        if !event.is_object() {
            return Err("Message de protocole invalide.");
        }
        let pending = self.pending.len();
        let serial = self.serial;
        let mut effects = match self.kind {
            Kind::Codex => self.codex(event)?,
            Kind::Claude => self.claude(event)?,
        };
        for text in &mut effects.text {
            *text = self.redact(text);
        }
        if let Some(summary) = &mut effects.summary {
            *summary = self.redact(summary);
        }
        if let Some(success) = effects.complete {
            self.finished = true;
            self.pending.clear();
            effects.complete = Some(success && !self.failed);
            effects.preview = Some(String::new());
        }
        effects.interactions_changed = pending != self.pending.len() || serial != self.serial;
        if effects.interactions_changed && effects.preview.is_none() {
            effects.preview = Some(self.preview(&json!(""))?);
        }
        Ok(effects)
    }
    fn codex(&mut self, event: Value) -> Result<Effects> {
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
                return Err("Demande reçue avant le démarrage de l’agent.");
            }
            let id = wire_id(&event["id"])?;
            let detail = serde_json::to_string_pretty(p).map_err(|_| "Demande invalide.")?;
            if detail.len() > 48 * 1024 {
                return Err("Demande de l’agent trop longue.");
            }
            if method == "item/tool/requestUserInput" {
                let questions = self.questions(&p["questions"])?;
                let original = p["questions"].as_array().ok_or("Questions invalides.")?;
                let mut unique = std::collections::BTreeSet::new();
                for question in original {
                    if !unique.insert(text(&question["id"], 128)?) {
                        return Err("Identifiant de question dupliqué.");
                    }
                }
                self.ask(
                    id,
                    method,
                    p.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: "L’agent attend votre réponse".into(),
                        detail: String::new(),
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
                    .map_err(|_| "Demande invalide.")?;
                if detail.len() > 64 * 1024 {
                    return Err("Demande de l’agent trop longue.");
                }
                self.ask(
                    id,
                    method,
                    p.clone(),
                    AgentInteraction {
                        id: String::new(),
                        title: "Autorisation demandée".into(),
                        detail,
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
                return Err("Erreur signalée par le protocole Codex.");
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
                        return Err(
                            "Codex doit confirmer un stockage des identifiants uniquement en mémoire avant d’utiliser cette clé API.",
                        );
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
                _ => return Err("Réponse de protocole inattendue."),
            }
        } else if method == "item/agentMessage/delta" {
            out.preview = Some(self.preview(&p["delta"])?);
        } else if method == "item/started" && p["item"]["type"] == "agentMessage" {
            self.preview.clear();
            out.preview = Some(String::new());
        } else if method == "item/started" && p["item"]["type"] == "commandExecution" {
            if let Some(command) = p["item"]["command"].as_str() {
                out.text.push(format!("→ {command}"));
            }
        } else if method == "item/started" && p["item"]["type"] == "fileChange" {
            if self.items.len() >= 32 {
                return Err("Trop de modifications simultanées.");
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
            out.usage = usage(
                &p["tokenUsage"]["total"],
                &["inputTokens", "cachedInputTokens", "outputTokens"],
            );
        } else if method == "error" && p["willRetry"] != true {
            self.failed = true;
            out.text.push("Erreur signalée par Codex.".into());
        } else if method == "turn/completed" {
            if self.phase != Phase::Running {
                return Err("Fin de tour inattendue.");
            }
            out.complete = Some(p["turn"]["status"] == "completed");
        }
        Ok(out)
    }
    fn claude(&mut self, event: Value) -> Result<Effects> {
        let mut out = Effects::default();
        match event["type"].as_str().unwrap_or("") {
            "control_response" if event["response"]["request_id"] == "initialize" => {
                if self.phase != Phase::Initializing || event["response"]["subtype"] != "success" {
                    return Err("Initialisation Claude impossible.");
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
                    return Err("Demande reçue avant le démarrage de l’agent.");
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
                    return Err("Paramètres d’outil invalides.");
                }
                let detail =
                    serde_json::to_string_pretty(input).map_err(|_| "Demande invalide.")?;
                if detail.len() > 48 * 1024 {
                    return Err("Demande de l’agent trop longue.");
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
                        detail: if questions.is_empty() {
                            detail
                        } else {
                            String::new()
                        },
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
                    return Err("Résultat Claude inattendu.");
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
    pub fn reply(&mut self, answer: AgentAnswer) -> Result<Effects> {
        if self.secrets.len()
            + answer
                .answers
                .values()
                .map(|values| values.len() * 4)
                .sum::<usize>()
            > 1024
            || self
                .secrets
                .iter()
                .map(String::len)
                .sum::<usize>()
                .saturating_add(
                    answer
                        .answers
                        .values()
                        .flatten()
                        .map(|value| value.len().saturating_mul(4))
                        .sum::<usize>(),
                )
                > 256 * 1024
        {
            return Err("Trop de réponses confidentielles dans cette session.");
        }
        let pending = self
            .pending
            .get(&answer.request_id)
            .ok_or("Cette demande n’est plus en attente.")?;
        let deny = answer.decision == "deny";
        if !deny {
            if pending.view.questions.is_empty() {
                if answer.decision != "allow"
                    || !pending.view.can_allow
                    || !answer.answers.is_empty()
                {
                    return Err("Autorisation invalide.");
                }
            } else {
                if answer.decision != "answer"
                    || answer.answers.len() != pending.view.questions.len()
                {
                    return Err("Réponds à chaque question.");
                }
                let mut size = 0;
                for question in &pending.view.questions {
                    let values = answer
                        .answers
                        .get(&question.id)
                        .filter(|values| {
                            !values.is_empty()
                                && values.len() <= if question.multi_select { 20 } else { 1 }
                        })
                        .ok_or("Réponds à chaque question.")?;
                    if values
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != values.len()
                    {
                        return Err("Réponse invalide.");
                    }
                    for value in values {
                        size += value.len();
                        if value.trim().is_empty() || value.len() > 4000 || value.contains('\0') {
                            return Err("Réponse invalide.");
                        }
                    }
                }
                if size > 32 * 1024 {
                    return Err("Réponse trop longue.");
                }
            }
        } else if !answer.answers.is_empty() {
            return Err("Réponse de refus invalide.");
        }
        let pending = self
            .pending
            .remove(&answer.request_id)
            .ok_or("Cette demande n’est plus en attente.")?;
        for question in &pending.view.questions {
            if question.is_secret {
                for value in answer.answers.get(&question.id).into_iter().flatten() {
                    self.protect(value);
                }
            }
        }
        let result = match self.kind {
            Kind::Codex => {
                let response = if pending.method == "item/tool/requestUserInput" {
                    let mut answers = serde_json::Map::new();
                    for (index, question) in pending.input["questions"]
                        .as_array()
                        .ok_or("Questions invalides.")?
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
                json!({"id":pending.wire_id,"result":response})
            }
            Kind::Claude => {
                let response = if deny {
                    json!({"behavior":"deny","message":"Demande refusée par l’utilisateur."})
                } else {
                    let mut input = pending.input;
                    if !pending.view.questions.is_empty() {
                        let questions = input["questions"]
                            .as_array()
                            .ok_or("Questions invalides.")?;
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
                json!({"type":"control_response","response":{"subtype":"success","request_id":pending.wire_id,"response":response}})
            }
        };
        Ok(Effects {
            send: vec![result],
            text: vec![if deny {
                "Demande refusée par l’utilisateur.".into()
            } else {
                "Réponse transmise à l’agent.".into()
            }],
            interactions_changed: true,
            preview: Some(self.preview(&json!(""))?),
            ..Effects::default()
        })
    }
}

fn usage(value: &Value, keys: &[&str; 3]) -> Option<Value> {
    Some(
        json!({"inputTokens":value[keys[0]].as_u64()?,"cachedInputTokens":value[keys[1]].as_u64(),"cacheCreationInputTokens":value["cache_creation_input_tokens"].as_u64(),"outputTokens":value[keys[2]].as_u64()?}),
    )
}
