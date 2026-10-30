use super::store::{Documents, Store};
use atelier_core::{
    application::task_templates::{Result, TemplateRepository},
    document::Document,
};

impl TemplateRepository for Store<'_> {
    fn card(&self, id: &str) -> Result<Option<Document>> {
        self.document(Documents::Cards, id)
    }
    fn settings(&self) -> Result<Document> {
        Store::settings(self)
    }
    fn save_settings(&self, settings: &Document) -> Result<()> {
        Store::save_settings(self, settings)
    }
}
