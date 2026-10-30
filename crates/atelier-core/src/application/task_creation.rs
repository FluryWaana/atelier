use super::card_lifecycle::CardFailure;
use crate::{
    contracts::Card,
    document::Document,
    domain::{agent_options::options_document, checklist::checklist},
};
use serde_json::{Value, value::RawValue};

use crate::Result;

pub trait CreationContext {
    fn settings(&self) -> Result<Document>;
    fn agent_for(&self, body: &Document) -> Result<String>;
    fn repository(&self, value: &RawValue) -> Result<String>;
    fn new_id(&self) -> String;
    fn timestamp(&self) -> Result<String>;
}

pub(super) fn text(value: Option<&RawValue>, limit: usize) -> Result<String> {
    let invalid = || CardFailure::Invalid("message.invalid_or_overly_long_text");
    let value = value.ok_or_else(invalid)?;
    let value = serde_json::from_str::<String>(value.get()).map_err(|_| invalid())?;
    Ok(crate::domain::text(&value, limit).map_err(CardFailure::Invalid)?)
}

pub(super) fn present(value: Option<&RawValue>) -> Option<&RawValue> {
    value.filter(|value| value.get() != "null")
}

pub fn default_agent(
    settings: &Document,
    projects: &[Document],
    body: &Document,
    windows: bool,
) -> Result<String> {
    let invalid = || CardFailure::Invalid("message.invalid_or_overly_long_text");
    let repository = |document: &Document| -> Result<Option<String>> {
        Ok(document
            .get::<Option<String>>("repo")
            .map_err(|_| invalid())?
            .flatten()
            .filter(|path| !path.is_empty()))
    };
    let repo = match repository(body)? {
        Some(repo) => repo,
        None => repository(settings)?.unwrap_or_default(),
    };
    for project in projects {
        let path = repository(project)?.unwrap_or_default();
        let matches = crate::domain::projects::same_repository(&path, &repo, windows);
        if matches {
            if let Some(agent) = project
                .get::<String>("agent")
                .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
                .filter(|agent| !agent.is_empty())
            {
                return Ok(agent);
            }
            break;
        }
    }
    let agent = settings
        .get::<String>("agent")
        .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
        .filter(|agent| !agent.is_empty())
        .unwrap_or_else(|| "codex".into());
    Ok(agent)
}

pub(super) fn provider(settings: &Document, agent: &str) -> Result<Document> {
    crate::domain::agent_options::find_provider(settings, agent)?
        .ok_or_else(|| CardFailure::Invalid("message.invalid_agent").into())
}

pub(super) fn options(agent: &str, body: &Document, settings: &Document) -> Result<Value> {
    let provider = provider(settings, agent)?;
    let kind = provider
        .get::<String>("type")
        .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
        .ok_or(CardFailure::Invalid("message.invalid_agent"))?;
    Ok(options_document(&kind, body).map_err(CardFailure::Invalid)?)
}

pub fn create(
    context: &impl CreationContext,
    body: &Document,
    brainstorming: bool,
) -> Result<Card> {
    let title = text(body.raw("title"), 300)?;
    if title.is_empty() {
        return Err(CardFailure::Invalid("message.a_title_is_required").into());
    }
    let template = if body
        .has_text("templateId")
        .map_err(|_| "message.invalid_request")?
    {
        let templates = context.settings()?;
        let mut found = None;
        if templates.has_list("taskTemplates")? {
            for template in templates
                .get::<Vec<Document>>("taskTemplates")?
                .unwrap_or_default()
            {
                if let (Ok(Some(id)), Ok(Some(selected))) = (
                    template.get::<String>("id"),
                    body.get::<String>("templateId"),
                ) && id == selected
                {
                    found = Some(template);
                    break;
                }
            }
        }
        Some(found.ok_or(CardFailure::Invalid("message.template_not_found"))?)
    } else {
        None
    };
    let agent = if body
        .has_text("agent")
        .map_err(|_| "message.invalid_agent")?
    {
        body.get::<String>("agent")
            .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
            .ok_or(CardFailure::Invalid("message.invalid_agent"))?
    } else {
        context.agent_for(body)?
    };
    let template = template.unwrap_or_default();
    let mode = if brainstorming {
        String::from("analyze")
    } else if body
        .has_text("mode")
        .map_err(|_| "message.invalid_agent_or_mode")?
    {
        body.get::<String>("mode")
            .ok()
            .flatten()
            .unwrap_or_default()
    } else if template.has_text("mode")? {
        template
            .get::<String>("mode")
            .ok()
            .flatten()
            .unwrap_or_default()
    } else {
        String::from("edit")
    };
    provider(&context.settings()?, &agent)?;
    if ![String::from("analyze"), String::from("edit")].contains(&mode) {
        return Err(CardFailure::Invalid("message.invalid_agent_or_mode").into());
    }
    let status = if brainstorming {
        String::from("todo")
    } else if let Some(value) = present(body.raw("status")) {
        serde_json::from_str::<String>(value.get()).unwrap_or_default()
    } else {
        String::from("todo")
    };
    if ![
        String::from("todo"),
        String::from("doing"),
        String::from("review"),
    ]
    .contains(&status)
    {
        return Err(CardFailure::Invalid("message.invalid_initial_status").into());
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
    if body
        .has_text("repo")
        .map_err(|_| "message.invalid_or_overly_long_text")?
    {
        fields.set(
            "repo",
            &context.repository(body.raw("repo").expect("validated text field exists"))?,
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
