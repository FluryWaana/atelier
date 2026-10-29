mod acp;
mod claude;
mod codex;

use crate::contracts::{AgentAnswer, AgentInteraction, AgentQuestion};
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Codex,
    Claude,
    Acp,
}

impl Kind {
    pub fn from_id(id: &str) -> Option<Self> {
        use crate::domain::agent_catalog::ProtocolKind;
        match crate::domain::agent_catalog::find(id)?.protocol {
            ProtocolKind::Codex => Some(Self::Codex),
            ProtocolKind::Claude => Some(Self::Claude),
            ProtocolKind::Acp => Some(Self::Acp),
            ProtocolKind::Command => None,
        }
    }

    pub fn launch_arguments(
        self,
        id: &str,
        edit: bool,
        model: &str,
        effort: &str,
        api_key: bool,
    ) -> Vec<String> {
        match self {
            Self::Codex => codex::launch_arguments(api_key),
            Self::Claude => claude::launch_arguments(edit, model, effort),
            Self::Acp => crate::domain::agent_catalog::find(id)
                .map(|agent| agent.launch_args.iter().map(|arg| (*arg).into()).collect())
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    CheckingCredentials,
    Authenticating,
    StartingThread,
    SettingOptions,
    SettingMode,
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
    codex_usage: Option<Value>,
    cost_eligible: bool,
    acp_setup: acp::Setup,
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
        .ok_or("message.protocol_message_invalid")
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
        Err("message.protocol_id_invalid")
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
            codex_usage: None,
            cost_eligible: true,
            acp_setup: acp::Setup::default(),
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
        let delta = delta.as_str().ok_or("message.public_fragment_invalid")?;
        if self.preview.len().saturating_add(delta.len()) > 1024 * 1024 {
            return Err("message.agent_response_too_long");
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
            return Err("message.session_already_initialized");
        }
        self.phase = Phase::Initializing;
        Ok(Effects {
            send: vec![match self.kind {
                Kind::Codex => codex::initialize(),
                Kind::Claude => claude::initialize(),
                Kind::Acp => acp::initialize(),
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
            return Err("message.agent_requests_invalid");
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
            .ok_or("message.agent_questions_invalid")?;
        let mut unique = std::collections::BTreeSet::new();
        for question in list {
            if !unique.insert(text(&question["question"], 4000)?) {
                return Err("message.question_duplicate");
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
                        return Err("message.agent_questions_invalid");
                    }
                }
                if value
                    .get("options")
                    .is_some_and(|value| !value.is_null() && !value.is_array())
                {
                    return Err("message.agent_questions_invalid");
                }
                let options = value.get("options").and_then(Value::as_array);
                if options.is_some_and(|options| options.len() > 20) {
                    return Err("message.agent_choices_too_many");
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
                    return Err("message.question_choice_duplicate");
                }
                Ok(AgentQuestion {
                    id: index.to_string(),
                    question: text(&value["question"], 4000)?,
                    options,
                    multi_select: value["multiSelect"] == true,
                    is_secret: value["isSecret"] == true,
                    required: true,
                })
            })
            .collect()
    }
    pub fn receive(&mut self, event: Value) -> Result<Effects> {
        if self.finished {
            return Ok(Effects::default());
        }
        if !event.is_object() {
            return Err("message.protocol_message_invalid");
        }
        let pending = self.pending.len();
        let serial = self.serial;
        let mut effects = match self.kind {
            Kind::Codex => self.codex(event)?,
            Kind::Claude => self.claude(event)?,
            Kind::Acp => self.acp(event)?,
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
            return Err("message.session_secret_answers_too_many");
        }
        let pending = self
            .pending
            .get(&answer.request_id)
            .ok_or("message.this_request_is_no_longer_pending")?;
        let deny = answer.decision == "deny";
        if !deny {
            if pending.view.questions.is_empty() {
                if answer.decision != "allow"
                    || !pending.view.can_allow
                    || !answer.answers.is_empty()
                {
                    return Err("message.invalid_approval");
                }
            } else {
                if answer.decision != "answer"
                    || answer.answers.len() != pending.view.questions.len()
                {
                    return Err("message.answer_every_question");
                }
                let mut size = 0;
                for question in &pending.view.questions {
                    let values = answer
                        .answers
                        .get(&question.id)
                        .filter(|values| values.len() <= if question.multi_select { 20 } else { 1 })
                        .ok_or("message.answer_every_question")?;
                    if question.required
                        && (values.is_empty() || values.iter().any(|value| value.trim().is_empty()))
                    {
                        return Err("message.answer_every_question");
                    }
                    if values
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != values.len()
                    {
                        return Err("message.invalid_reply");
                    }
                    for value in values {
                        size += value.len();
                        if value.len() > 4000 || value.contains('\0') {
                            return Err("message.invalid_reply");
                        }
                    }
                }
                if size > 32 * 1024 {
                    return Err("message.reply_is_too_long");
                }
            }
        } else if !answer.answers.is_empty() {
            return Err("message.decline_response_invalid");
        }
        let acp_result = if self.kind == Kind::Acp && pending.method == "elicitation/create" {
            Some(acp::elicitation_reply(pending, &answer, deny)?)
        } else {
            None
        };
        let pending = self
            .pending
            .remove(&answer.request_id)
            .ok_or("message.this_request_is_no_longer_pending")?;
        for question in &pending.view.questions {
            if question.is_secret {
                for value in answer.answers.get(&question.id).into_iter().flatten() {
                    self.protect(value);
                }
            }
        }
        let result = match self.kind {
            Kind::Codex => codex::reply(pending, &answer, deny)?,
            Kind::Claude => claude::reply(pending, &answer, deny)?,
            Kind::Acp => match acp_result {
                Some(result) => result,
                None => acp::reply(pending, deny)?,
            },
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
