use super::store::{Documents, Store};
use atelier_core::{
    application::{task_creation::Result, task_editing::EditingContext},
    document::Document,
};

impl EditingContext for Store<'_> {
    fn cards(&self) -> Result<Vec<Document>> {
        self.documents(Documents::Cards)
    }
}
