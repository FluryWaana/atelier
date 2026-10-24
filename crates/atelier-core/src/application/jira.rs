use crate::contracts::{JiraConfiguration, JiraConfigurationUpdate, JiraMode};

#[derive(Debug)]
pub enum JiraFailure {
    Conflict,
    Invalid(&'static str),
}
impl From<&'static str> for JiraFailure {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}
impl std::fmt::Display for JiraFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Conflict => {
                "La connexion Jira a changé. Recharge les réglages avant de réessayer."
            }
            Self::Invalid(message) => message,
        })
    }
}
impl std::error::Error for JiraFailure {}

pub fn configure(
    previous: &JiraConfiguration,
    input: JiraConfigurationUpdate,
    normalize_url: impl FnOnce(&str) -> Result<String, &'static str>,
) -> Result<(JiraConfiguration, Option<String>), JiraFailure> {
    if previous.revision != input.expected_revision {
        return Err(JiraFailure::Conflict);
    }
    if input.email.len() > 300
        || input.email.chars().any(char::is_control)
        || input.query.len() > 16000
        || input.query.chars().count() > 4000
        || input.query.contains('\0')
    {
        return Err("Paramètres Jira invalides.".into());
    }
    let url = if input.url.trim().is_empty() {
        String::new()
    } else {
        normalize_url(input.url.trim())?
    };
    let email = input.email.trim().to_owned();
    if input.mode == JiraMode::Cloud
        && !url.is_empty()
        && (email.is_empty() || !email.contains('@') || email.contains(':'))
    {
        return Err("Renseigne l’adresse e-mail du compte Jira Cloud.".into());
    }
    let token = input.token.filter(|token| !token.is_empty());
    if token
        .as_ref()
        .is_some_and(|token| token.len() > 4000 || token.chars().any(char::is_control))
        || (input.clear_token && token.is_some())
    {
        return Err("Jeton Jira invalide.".into());
    }
    let scope_changed =
        previous.url != url || previous.email != email || previous.mode != input.mode;
    let token = if input.clear_token {
        Some(String::new())
    } else {
        token.or_else(|| scope_changed.then(String::new))
    };
    Ok((
        JiraConfiguration {
            mode: input.mode,
            url,
            email,
            query: input.query.trim().into(),
            revision: previous
                .revision
                .checked_add(1)
                .ok_or("Révision Jira épuisée.")?,
        },
        token,
    ))
}

pub fn issue_identity(id: &str, key: &str) -> bool {
    let Some((project, number)) = key.rsplit_once('-') else {
        return false;
    };
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !project.is_empty()
        && project.len() <= 64
        && project.as_bytes()[0].is_ascii_alphabetic()
        && project
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !number.is_empty()
        && number.len() <= 20
        && number.bytes().all(|byte| byte.is_ascii_digit())
}
