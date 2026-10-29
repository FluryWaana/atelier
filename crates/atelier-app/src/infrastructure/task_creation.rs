use super::{
    store::{Documents, Store},
    time_tracking::Clock,
};
use atelier_core::{
    application::{
        card_lifecycle::CardFailure,
        task_creation::{CreationContext, Result, default_agent},
        time_tracking::TimeClock,
    },
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::value::RawValue;

impl CreationContext for Store<'_> {
    fn settings(&self) -> Result<Document> {
        let mut settings = self.settings::<Document>()?;
        if settings.raw("repo").is_none() {
            settings.set("repo", &"")?;
        }
        Ok(settings)
    }
    fn agent_for(&self, body: &Document) -> Result<JsonString> {
        default_agent(
            &CreationContext::settings(self)?,
            &self.documents::<Document>(Documents::Projects)?,
            body,
            cfg!(windows),
        )
    }
    fn repository(&self, value: &RawValue) -> Result<JsonString> {
        let value: JsonString = serde_json::from_str(value.get())
            .map_err(|_| CardFailure::Invalid("message.invalid_or_overly_long_text"))?;
        let value = text_units(&value, 2000).map_err(CardFailure::Invalid)?;
        let value = String::from_utf16_lossy(&value.0);
        let unavailable =
            |_| CardFailure::Invalid("message.the_repository_could_not_be_found_or_accessed");
        let path = std::fs::canonicalize(if value.is_empty() { "." } else { &value })
            .map_err(unavailable)?;
        if !path.metadata().map_err(unavailable)?.is_dir() {
            return Err(CardFailure::Invalid("message.the_repository_must_be_a_folder").into());
        }
        Ok(path_string(&path))
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
    fn timestamp(&self) -> Result<String> {
        Clock.format(Clock.now())
    }
}

pub(crate) fn path_string(path: &std::path::Path) -> JsonString {
    let path = path.to_string_lossy();
    #[cfg(windows)]
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    JsonString::from(path.as_ref())
}
