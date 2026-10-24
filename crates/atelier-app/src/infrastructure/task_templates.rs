use super::store::{Documents, Store};
use atelier_core::{
    application::task_templates::{Result, TemplateRepository},
    document::Document,
};
use serde_json::value::RawValue;

impl TemplateRepository for Store<'_> {
    fn card(&self, id: Option<&RawValue>) -> Result<Option<Document>> {
        self.document_input(Documents::Cards, id)
    }
    fn settings(&self) -> Result<Document> {
        Store::settings(self)
    }
    fn save_settings(&self, settings: &Document) -> Result<()> {
        Store::save_settings(self, settings)
    }
}
