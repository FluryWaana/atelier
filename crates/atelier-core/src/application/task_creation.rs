use super::card_lifecycle::CardFailure;
use crate::{
    contracts::Card,
    document::{Document, JsonString},
    domain::{agent_options::agent_options, checklist::checklist, text_units},
};
use serde_json::{Value, value::RawValue};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub trait CreationContext {
    fn settings(&self) -> Result<Document>;
    fn agent_for(&self, body: &Document) -> Result<JsonString>;
    fn repository(&self, value: &RawValue) -> Result<JsonString>;
    fn new_id(&self) -> String;
    fn timestamp(&self) -> Result<String>;
}

pub(super) fn text(value: Option<&RawValue>, limit: usize) -> Result<JsonString> {
    let invalid = || CardFailure::Invalid("Texte invalide ou trop long.");
    let value = value.ok_or_else(invalid)?;
    let value = serde_json::from_str::<JsonString>(value.get()).map_err(|_| invalid())?;
    Ok(text_units(&value, limit).map_err(CardFailure::Invalid)?)
}

pub(super) fn present(value: Option<&RawValue>) -> Option<&RawValue> {
    value.filter(|value| value.get() != "null")
}

pub fn default_agent(
    settings: &Document,
    projects: &[Document],
    body: &Document,
    windows: bool,
) -> Result<JsonString> {
    let empty = RawValue::from_string("\"\"".into())?;
    let repo = if body.truthy("repo")? {
        body.raw("repo")
    } else if settings.truthy("repo")? {
        settings.raw("repo")
    } else {
        None
    }
    .unwrap_or(&empty);
    for project in projects {
        let path = if project.truthy("repo")? {
            project.raw("repo")
        } else {
            None
        }
        .unwrap_or(&empty);
        let matches = if windows || (path.get().starts_with('"') && repo.get().starts_with('"')) {
            let invalid = "(value || \"\").replaceAll is not a function";
            let path = serde_json::from_str::<JsonString>(path.get()).map_err(|_| invalid)?;
            let repo = serde_json::from_str::<JsonString>(repo.get()).map_err(|_| invalid)?;
            crate::domain::projects::same_repository_units(&path, &repo, windows)
        } else if [path, repo]
            .iter()
            .any(|raw| raw.get().starts_with(['{', '[', '"']))
        {
            false
        } else {
            serde_json::from_str::<Value>(path.get())? == serde_json::from_str::<Value>(repo.get())?
        };
        if matches {
            if project.truthy("agent")? {
                return Ok(project
                    .get::<JsonString>("agent")
                    .map_err(|_| CardFailure::Invalid("Agent invalide."))?
                    .ok_or(CardFailure::Invalid("Agent invalide."))?);
            }
            break;
        }
    }
    if settings.truthy("agent")? {
        Ok(settings
            .get::<JsonString>("agent")
            .map_err(|_| CardFailure::Invalid("Agent invalide."))?
            .ok_or(CardFailure::Invalid("Agent invalide."))?)
    } else {
        Ok(JsonString::from("codex"))
    }
}

pub(super) fn provider(settings: &Document, agent: &JsonString) -> Result<Document> {
    crate::domain::agent_options::find_provider(settings, agent)?
        .ok_or_else(|| CardFailure::Invalid("Agent invalide.").into())
}

pub(super) fn options(agent: &JsonString, body: &Document, settings: &Document) -> Result<Value> {
    let provider = provider(settings, agent)?;
    let kind = provider
        .get::<String>("type")
        .map_err(|_| CardFailure::Invalid("Agent invalide."))?
        .ok_or(CardFailure::Invalid("Agent invalide."))?;
    let mut options = serde_json::Map::new();
    for key in ["model", "effort"] {
        if let Some(raw) = body.raw(key) {
            // These ASCII-only options reject lone surrogates; keep the validator's error order.
            options.insert(
                key.into(),
                serde_json::from_str(raw.get()).unwrap_or(Value::Null),
            );
        }
    }
    Ok(agent_options(&kind, &Value::Object(options), None).map_err(CardFailure::Invalid)?)
}

pub fn create(
    context: &impl CreationContext,
    body: &Document,
    brainstorming: bool,
) -> Result<Card> {
    let title = text(body.raw("title"), 300)?;
    if title.0.is_empty() {
        return Err(CardFailure::Invalid("Un titre est nécessaire.").into());
    }
    let template = if body.truthy("templateId")? {
        let templates = context.settings()?;
        let mut found = None;
        if templates.truthy("taskTemplates")? {
            for template in templates
                .get::<Vec<Document>>("taskTemplates")?
                .unwrap_or_default()
            {
                if let (Ok(Some(id)), Ok(Some(selected))) = (
                    template.get::<JsonString>("id"),
                    body.get::<JsonString>("templateId"),
                ) && id == selected
                {
                    found = Some(template);
                    break;
                }
            }
        }
        Some(found.ok_or(CardFailure::Invalid("Modèle introuvable."))?)
    } else {
        None
    };
    let agent = if body.truthy("agent")? {
        body.get::<JsonString>("agent")
            .map_err(|_| CardFailure::Invalid("Agent invalide."))?
            .ok_or(CardFailure::Invalid("Agent invalide."))?
    } else {
        context.agent_for(body)?
    };
    let template = template.unwrap_or_default();
    let mode = if brainstorming {
        JsonString::from("analyze")
    } else if body.truthy("mode")? {
        body.get::<JsonString>("mode")
            .ok()
            .flatten()
            .unwrap_or_default()
    } else if template.truthy("mode")? {
        template
            .get::<JsonString>("mode")
            .ok()
            .flatten()
            .unwrap_or_default()
    } else {
        JsonString::from("edit")
    };
    provider(&context.settings()?, &agent)?;
    if ![JsonString::from("analyze"), JsonString::from("edit")].contains(&mode) {
        return Err(CardFailure::Invalid("Agent ou mode invalide.").into());
    }
    let status = if brainstorming {
        JsonString::from("todo")
    } else if let Some(value) = present(body.raw("status")) {
        serde_json::from_str::<JsonString>(value.get()).unwrap_or_default()
    } else {
        JsonString::from("todo")
    };
    if ![
        JsonString::from("todo"),
        JsonString::from("doing"),
        JsonString::from("review"),
    ]
    .contains(&status)
    {
        return Err(CardFailure::Invalid("Statut initial invalide.").into());
    }
    let id = context.new_id();
    let empty = RawValue::from_string("\"\"".into())?;
    let mut fields = Document::default();
    for key in ["description", "instructions"] {
        fields.set(
            key,
            &text(
                present(body.raw(key))
                    .or_else(|| present(template.raw(key)))
                    .or(Some(&empty)),
                20_000,
            )?,
        )?;
    }
    if body.truthy("repo")? {
        fields.set(
            "repo",
            &context.repository(body.raw("repo").expect("truthy field exists"))?,
        )?;
    } else if brainstorming {
        fields.set("repo", &"")?;
    } else if let Some(repo) = context.settings()?.raw("repo") {
        fields.set("repo", &repo)?;
    }
    fields.set("agent", &agent)?;
    let options = options(&agent, body, &context.settings()?)?;
    for key in ["model", "effort"] {
        fields.set(key, &options[key])?;
    }
    fields.set("mode", &mode)?;
    let empty_list = RawValue::from_string("[]".into())?;
    let criteria = present(body.raw("checklist"))
        .or_else(|| present(template.raw("checklist")))
        .unwrap_or(&empty_list);
    fields.set(
        "checklist",
        &checklist(criteria).map_err(CardFailure::Invalid)?,
    )?;
    fields.set("status", &status)?;
    if brainstorming {
        fields.set("brainstormSession", &true)?;
    }
    fields.set("createdAt", &context.timestamp()?)?;
    fields.set("updatedAt", &context.timestamp()?)?;
    Ok(Card { id, title, fields })
}
