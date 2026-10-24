use super::store::{Documents, Store};
use atelier_core::{
    application::{projects::ProjectRepository, task_creation::Result},
    document::Document,
};

impl ProjectRepository for Store<'_> {
    fn projects(&self) -> Result<Vec<Document>> {
        self.documents(Documents::Projects)
    }
    fn cards(&self) -> Result<Vec<Document>> {
        self.documents(Documents::Cards)
    }
    fn times(&self) -> Result<Vec<Document>> {
        self.documents(Documents::Times)
    }
    fn save_project(&self, project: &Document) -> Result<()> {
        self.put(Documents::Projects, project)
    }
    fn save_card(&self, card: &Document) -> Result<()> {
        self.put(Documents::Cards, card)
    }
    fn delete_project(&self, id: &str) -> Result<()> {
        self.delete(Documents::Projects, id)
    }
}
