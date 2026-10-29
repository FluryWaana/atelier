use super::card_lifecycle::CardFailure;
use crate::{
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::value::RawValue;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub trait TemplateRepository {
    fn card(&self, id: Option<&RawValue>) -> Result<Option<Document>>;
    fn settings(&self) -> Result<Document>;
    fn save_settings(&self, settings: &Document) -> Result<()>;
}

fn templates(settings: &Document) -> Result<Vec<Document>> {
    Ok(if settings.truthy("taskTemplates")? {
        settings.get("taskTemplates")?.unwrap_or_default()
    } else {
        Vec::new()
    })
}

pub fn create(
    repository: &impl TemplateRepository,
    body: &Document,
    new_id: impl FnOnce() -> String,
) -> Result<Document> {
    let invalid = CardFailure::Invalid("message.invalid_or_overly_long_text");
    let name = body
        .get::<JsonString>("name")
        .ok()
        .flatten()
        .ok_or(invalid)?;
    let name = text_units(&name, 100).map_err(CardFailure::Invalid)?;
    if name.0.is_empty() {
        return Err(CardFailure::Invalid("message.template_name_required").into());
    }
    let card = repository
        .card(body.raw("cardId"))?
        .ok_or(CardFailure::Missing)?;
    let mut settings = repository.settings()?;
    let mut templates = templates(&settings)?;
    if templates.len() >= 50 {
        return Err(
            CardFailure::Invalid("message.remove_a_template_before_adding_another_50_maximum").into(),
        );
    }
    let mut template = Document::default();
    template.set("id", &new_id())?;
    template.set("name", &name)?;
    for (key, default) in [("description", ""), ("instructions", ""), ("mode", "edit")] {
        if card.truthy(key)? {
            if let Some(value) = card.raw(key) {
                template.set(key, &value)?;
            }
        } else {
            template.set(key, &default)?;
        }
    }
    template.set("checklist", &super::reset_checklist(&card)?)?;
    templates.push(template.clone());
    settings.set("taskTemplates", &templates)?;
    repository.save_settings(&settings)?;
    Ok(template)
}

pub fn remove(repository: &impl TemplateRepository, id: &str) -> Result<()> {
    let mut settings = repository.settings()?;
    let mut kept = Vec::new();
    for template in templates(&settings)? {
        if !template.string_equals("id", id)? {
            kept.push(template);
        }
    }
    settings.set("taskTemplates", &kept)?;
    repository.save_settings(&settings)
}
