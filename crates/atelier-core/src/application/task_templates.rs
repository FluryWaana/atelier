use super::card_lifecycle::CardFailure;
use crate::{document::Document, domain::text};

use crate::Result;

pub trait TemplateRepository {
    fn card(&self, id: &str) -> Result<Option<Document>>;
    fn settings(&self) -> Result<Document>;
    fn save_settings(&self, settings: &Document) -> Result<()>;
}

fn templates(settings: &Document) -> Result<Vec<Document>> {
    Ok(if settings.has_list("taskTemplates")? {
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
    let name = body.get::<String>("name").ok().flatten().ok_or(invalid)?;
    let name = text(&name, 100).map_err(CardFailure::Invalid)?;
    if name.is_empty() {
        return Err(CardFailure::Invalid("message.template_name_required").into());
    }
    let card_id = body
        .get::<String>("cardId")
        .map_err(|_| CardFailure::Invalid("message.invalid_card_id"))?
        .filter(|id| !id.trim().is_empty())
        .ok_or(CardFailure::Invalid("message.invalid_card_id"))?;
    let card = repository.card(&card_id)?.ok_or(CardFailure::Missing)?;
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
        if card.has_text(key)? {
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
