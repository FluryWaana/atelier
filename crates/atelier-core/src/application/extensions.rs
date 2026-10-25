use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const PROTOCOL: &str = "atelier-connector/1";
pub const INPUT_LIMIT: usize = 256 * 1024;
pub const OUTPUT_LIMIT: usize = 512 * 1024;
pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binary {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Describe,
    Check,
    TicketsSearch,
    TicketsRead,
    TicketsReset,
    TicketsCreate,
    TicketsTransitions,
    TicketsTransition,
    RepositoriesPreview,
    RepositoriesPublish,
    RepositoriesRefresh,
    RepositoriesReview,
    TimePreview,
    TimeSend,
    TimeReconcile,
    Action,
    Reconcile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    Read,
    Local,
    Remote,
}
impl Method {
    pub fn effect(self) -> Effect {
        match self {
            Self::TicketsCreate
            | Self::TicketsTransition
            | Self::RepositoriesPublish
            | Self::TimeSend => Effect::Remote,
            Self::TicketsReset => Effect::Local,
            _ => Effect::Read,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Text,
    Url,
    Password,
    Checkbox,
    Select,
    Textarea,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default)]
    pub credential_scope: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub effect: Effect,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub id: String,
    pub label: String,
    pub description: String,
    pub actions: Vec<Action>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source_locale: String,
    pub methods: Vec<Method>,
    pub fields: Vec<Field>,
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    pub request_id: String,
    pub module: String,
    pub method: Method,
    pub operation_id: Option<String>,
    pub action: Option<String>,
    pub settings: BTreeMap<String, Value>,
    pub secrets: BTreeMap<String, String>,
    pub input: Value,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ok,
    Error,
    Uncertain,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reply {
    pub protocol: String,
    pub request_id: String,
    pub module: String,
    pub method: Method,
    pub status: Status,
    pub message: String,
    pub output: Value,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
fn text(value: &str, limit: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.encode_utf16().count() <= limit
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if !identifier(&manifest.id)
        || [
            "jira", "azure", "tempo", "github", "gitlab", "codex", "claude",
        ]
        .contains(&manifest.id.as_str())
        || !text(&manifest.name, 120, false)
        || !text(&manifest.description, 2000, true)
        || !["fr-FR", "en-US"].contains(&manifest.source_locale.as_str())
        || manifest.fields.len() > 32
        || manifest.tabs.len() > 8
        || manifest.methods.len() > 32
    {
        return Err("Manifeste de connecteur invalide.");
    }
    let methods = manifest.methods.iter().copied().collect::<BTreeSet<_>>();
    if methods.len() != manifest.methods.len()
        || !methods.contains(&Method::Check)
        || methods.contains(&Method::Describe)
    {
        return Err("Méthodes du connecteur invalides ou répétées.");
    }
    if methods
        .iter()
        .any(|method| method.effect() == Effect::Remote)
        && !methods.contains(&Method::Reconcile)
    {
        return Err("Un connecteur qui écrit à distance doit proposer le rapprochement.");
    }
    let mut ids = BTreeSet::new();
    for field in &manifest.fields {
        let invalid_options = match field.kind {
            FieldKind::Select => field.options.is_empty(),
            _ => !field.options.is_empty(),
        };
        if !identifier(&field.id)
            || !ids.insert(&field.id)
            || !text(&field.label, 200, false)
            || field.options.len() > 100
            || invalid_options
            || (field.kind == FieldKind::Password && field.credential_scope)
        {
            return Err("Champ de connecteur invalide ou répété.");
        }
        let mut values = BTreeSet::new();
        for option in &field.options {
            if !text(&option.value, 4000, true)
                || !text(&option.label, 200, false)
                || !values.insert(&option.value)
            {
                return Err("Choix de connecteur invalide ou répété.");
            }
        }
    }
    let mut tabs = BTreeSet::new();
    let mut actions = BTreeSet::new();
    for tab in &manifest.tabs {
        if !identifier(&tab.id)
            || !tabs.insert(&tab.id)
            || !text(&tab.label, 200, false)
            || !text(&tab.description, 2000, true)
            || tab.actions.len() > 16
        {
            return Err("Onglet de connecteur invalide.");
        }
        for action in &tab.actions {
            if !identifier(&action.id)
                || !actions.insert(&action.id)
                || !text(&action.label, 200, false)
                || (action.effect == Effect::Remote && !methods.contains(&Method::Reconcile))
            {
                return Err("Action de connecteur invalide ou répétée.");
            }
        }
    }
    if methods.contains(&Method::Action) {
        if actions.is_empty() {
            return Err("Actions du connecteur incohérentes.");
        }
    } else if !actions.is_empty() {
        return Err("Actions du connecteur incohérentes.");
    }
    if manifest.translations.len() > 2 {
        return Err("Catalogue de connecteur trop volumineux.");
    }
    for (locale, messages) in &manifest.translations {
        if !["fr-FR", "en-US"].contains(&locale.as_str())
            || messages.len() > 200
            || messages
                .iter()
                .any(|(key, value)| !text(key, 2000, false) || !text(value, 4000, true))
        {
            return Err("Catalogue de connecteur invalide.");
        }
    }
    if serde_json::to_vec(manifest)
        .map_err(|_| "Manifeste de connecteur invalide.")?
        .len()
        > 128 * 1024
    {
        return Err("Manifeste de connecteur trop volumineux.");
    }
    Ok(())
}

pub fn effect(manifest: &Manifest, request: &Request) -> Result<Effect> {
    if request.protocol != PROTOCOL
        || request.module != manifest.id
        || !identifier(&request.request_id)
        || request
            .operation_id
            .as_ref()
            .is_some_and(|id| !identifier(id))
    {
        return Err("Identité de requête de connecteur invalide.");
    }
    if request.method == Method::Describe {
        if request.action.is_some()
            || request.operation_id.is_some()
            || !request.settings.is_empty()
            || !request.secrets.is_empty()
            || request.input != Value::Null
        {
            return Err("La découverte du connecteur ne reçoit aucun contexte ni secret.");
        }
        return Ok(Effect::Read);
    }
    if !manifest.methods.contains(&request.method) {
        return Err("Méthode non proposée par ce connecteur.");
    }
    if serde_json::to_vec(request)
        .map_err(|_| "Requête de connecteur invalide.")?
        .len()
        > INPUT_LIMIT
    {
        return Err("Requête de connecteur trop volumineuse.");
    }
    for (key, value) in &request.settings {
        let field = manifest
            .fields
            .iter()
            .find(|field| &field.id == key && field.kind != FieldKind::Password)
            .ok_or("Paramètre public de connecteur inconnu.")?;
        if field.kind == FieldKind::Checkbox {
            if !value.is_boolean() {
                return Err("Valeur de connecteur invalide.");
            }
        } else {
            let value = value
                .as_str()
                .filter(|value| text(value, 4000, true))
                .ok_or("Valeur de connecteur invalide.")?;
            if field.kind == FieldKind::Select
                && !field.options.iter().any(|option| option.value == value)
            {
                return Err("Choix de connecteur invalide ou répété.");
            }
        }
    }
    for (key, value) in &request.secrets {
        if !manifest
            .fields
            .iter()
            .any(|field| &field.id == key && field.kind == FieldKind::Password)
            || value.is_empty()
            || value.len() > 4000
            || value.contains(['\0', '\r', '\n'])
        {
            return Err("Secret de connecteur invalide ou non déclaré.");
        }
    }
    let effect = if request.method == Method::Action {
        manifest
            .tabs
            .iter()
            .flat_map(|tab| &tab.actions)
            .find(|action| Some(&action.id) == request.action.as_ref())
            .map(|action| action.effect)
            .ok_or("Action de connecteur inconnue.")?
    } else {
        if request.action.is_some() {
            return Err("Action de connecteur inattendue.");
        }
        request.method.effect()
    };
    if effect != Effect::Read && request.operation_id.is_none() {
        return Err("Toute écriture de connecteur exige une intention persistante.");
    }
    if matches!(request.method, Method::Reconcile | Method::TimeReconcile)
        && request.operation_id.is_none()
    {
        return Err("Le rapprochement exige l’identité de l’opération d’origine.");
    }
    Ok(effect)
}

pub fn decode_reply(bytes: &[u8], request: &Request) -> Result<Reply> {
    if bytes.len() > OUTPUT_LIMIT {
        return Err("Réponse de connecteur trop volumineuse.");
    }
    let reply: Reply = serde_json::from_slice(bytes)
        .map_err(|_| "Réponse de connecteur invalide ou sortie supplémentaire.")?;
    if reply.protocol != PROTOCOL
        || reply.request_id != request.request_id
        || reply.module != request.module
        || reply.method != request.method
        || !text(&reply.message, 2000, true)
    {
        return Err("Réponse de connecteur liée à une autre requête.");
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        Manifest {
            id: "synthetic-demo".into(),
            name: "Synthetic connector".into(),
            description: String::new(),
            source_locale: "fr-FR".into(),
            methods: vec![Method::Check],
            fields: vec![],
            tabs: vec![],
            translations: BTreeMap::new(),
        }
    }
    fn request() -> Request {
        Request {
            protocol: PROTOCOL.into(),
            request_id: "request-synthetic-1".into(),
            module: "synthetic-demo".into(),
            method: Method::Check,
            operation_id: None,
            action: None,
            settings: BTreeMap::new(),
            secrets: BTreeMap::new(),
            input: Value::Null,
        }
    }
    #[test]
    fn manifests_refuse_ambiguous_fields_actions_and_writes_without_reconciliation() {
        let valid = manifest();
        assert!(validate_manifest(&valid).is_ok());
        let mut bad = valid.clone();
        bad.methods.push(Method::TimeSend);
        assert!(validate_manifest(&bad).is_err());
        bad.methods.push(Method::Reconcile);
        assert!(validate_manifest(&bad).is_ok());
        bad.methods.push(Method::Check);
        assert!(validate_manifest(&bad).is_err());
        let mut bad = valid.clone();
        bad.fields.push(Field {
            id: "scope".into(),
            label: "Scope".into(),
            kind: FieldKind::Select,
            options: vec![],
            credential_scope: true,
        });
        assert!(validate_manifest(&bad).is_err());
        bad.fields[0].options.push(Choice {
            value: "synthetic".into(),
            label: "Synthetic".into(),
        });
        assert!(validate_manifest(&bad).is_ok());
        bad.fields.push(bad.fields[0].clone());
        assert!(validate_manifest(&bad).is_err());
        let mut bad = valid;
        bad.tabs.push(Tab {
            id: "diagnostic".into(),
            label: "Diagnostic".into(),
            description: String::new(),
            actions: vec![Action {
                id: "write".into(),
                label: "Write".into(),
                effect: Effect::Remote,
            }],
        });
        bad.methods.push(Method::Action);
        assert!(validate_manifest(&bad).is_err());
    }
    #[test]
    fn replies_are_bound_to_the_request_and_a_partial_or_multiple_frame_never_qualifies() {
        let request = request();
        assert_eq!(effect(&manifest(), &request), Ok(Effect::Read));
        let reply = Reply {
            protocol: PROTOCOL.into(),
            request_id: request.request_id.clone(),
            module: request.module.clone(),
            method: request.method,
            status: Status::Ok,
            message: "Synthetic result".into(),
            output: Value::Null,
        };
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(decode_reply(&bytes, &request).is_ok());
        let mut other = request.clone();
        other.request_id = "other".into();
        assert!(decode_reply(&bytes, &other).is_err());
        let mut extra = bytes.clone();
        extra.extend_from_slice(&bytes);
        assert!(decode_reply(&extra, &request).is_err());
        assert!(decode_reply(&bytes[..bytes.len() - 1], &request).is_err());
        let mut write = request;
        write.method = Method::TimeSend;
        let mut manifest = manifest();
        manifest
            .methods
            .extend([Method::TimeSend, Method::Reconcile]);
        assert!(effect(&manifest, &write).is_err());
        write.operation_id = Some("operation-synthetic".into());
        assert_eq!(effect(&manifest, &write), Ok(Effect::Remote));
    }
    #[test]
    fn configured_values_do_not_coerce_types_or_promote_passwords_to_public_settings() {
        let mut manifest = manifest();
        manifest.fields.extend([
            Field {
                id: "token".into(),
                label: "Token".into(),
                kind: FieldKind::Password,
                options: vec![],
                credential_scope: false,
            },
            Field {
                id: "enabled".into(),
                label: "Enabled".into(),
                kind: FieldKind::Checkbox,
                options: vec![],
                credential_scope: true,
            },
        ]);
        let mut supplied = request();
        supplied
            .settings
            .insert("enabled".into(), Value::Bool(false));
        supplied
            .secrets
            .insert("token".into(), "synthetic-secret".into());
        assert_eq!(effect(&manifest, &supplied), Ok(Effect::Read));
        supplied
            .settings
            .insert("token".into(), Value::String("must-never-be-public".into()));
        assert!(effect(&manifest, &supplied).is_err());
        supplied.settings.remove("token");
        supplied
            .settings
            .insert("enabled".into(), Value::String("false".into()));
        assert!(effect(&manifest, &supplied).is_err());
        supplied
            .settings
            .insert("enabled".into(), Value::Bool(false));
        supplied
            .secrets
            .insert("undeclared".into(), "synthetic-secret".into());
        assert!(effect(&manifest, &supplied).is_err());
        supplied.secrets.remove("undeclared");
        supplied.input = Value::String("x".repeat(INPUT_LIMIT));
        assert!(effect(&manifest, &supplied).is_err());
    }
}
