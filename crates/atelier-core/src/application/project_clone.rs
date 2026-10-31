use crate::{Result, application::projects::ProjectFailure, contracts::ForgeKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloneRequest {
    pub url: String,
    pub parent: String,
    pub directory: String,
    pub connection: Option<ForgeKind>,
    pub operation: String,
    pub confirmed: bool,
}

impl CloneRequest {
    pub fn validate(&self) -> Result<()> {
        let invalid = || ProjectFailure::Invalid("message.clone_invalid_request");
        if !self.confirmed
            || self.url.is_empty()
            || self.url.len() > 2000
            || self
                .url
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || self.parent.trim().is_empty()
            || self.parent.encode_utf16().count() > 1800
            || self.parent.chars().any(char::is_control)
            || self.directory.len() > 100
            || self.directory.contains('/')
            || self.directory.starts_with('.')
            || self.directory.chars().any(char::is_control)
            || self.operation.is_empty()
            || self.operation.len() > 100
            || !self
                .operation
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(invalid().into());
        }
        crate::domain::files::validate_path(&self.directory, true).map_err(|_| invalid())?;
        Ok(())
    }
}

pub fn suggested_directory(url: &str) -> String {
    let name = url
        .trim()
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    let name = name.strip_suffix(".git").unwrap_or(name);
    if name.len() <= 100
        && !name.starts_with('.')
        && crate::domain::files::validate_path(name, true).is_ok()
    {
        name.into()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destination_is_a_single_portable_new_directory() {
        let mut input = CloneRequest {
            url: "https://forge.example.test/team/repo.git".into(),
            parent: "/synthetic".into(),
            directory: "repo".into(),
            connection: None,
            operation: "clone-123".into(),
            confirmed: true,
        };
        input.validate().unwrap();
        for name in [
            "",
            "../outside",
            "a/b",
            "a\\b",
            "C:repo",
            "NUL",
            "repo.",
            ".git",
            "repo\n",
        ] {
            input.directory = name.into();
            assert!(input.validate().is_err(), "{name:?}");
        }
        assert_eq!(
            suggested_directory("git@forge.example.test:team/repo.git"),
            "repo"
        );
        assert_eq!(
            suggested_directory("https://forge.example.test/team/repo.git/"),
            "repo"
        );
        assert_eq!(
            suggested_directory("https://forge.example.test/team/NUL.git"),
            ""
        );
    }
}
