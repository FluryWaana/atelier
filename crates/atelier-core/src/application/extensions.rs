use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const PROTOCOL: &str = "atelier-connector/1";
pub const INPUT_LIMIT: usize = 256 * 1024;
pub const OUTPUT_LIMIT: usize = 512 * 1024;
pub type Result<T> = std::result::Result<T, &'static str>;
#[path = "extension_repositories.rs"]
mod repositories;
pub use repositories::*;
#[path = "extension_ticket_reset.rs"]
mod ticket_reset;
pub use ticket_reset::*;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binary {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Installed {
    pub manifest: Manifest,
    pub binary: Binary,
    pub revision: u64,
    pub values: BTreeMap<String, Value>,
    pub report: Option<Report>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub id: String,
    pub method: Method,
    pub action: Option<String>,
    pub message: String,
    pub output: Value,
    pub observed_at: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub installed: Installed,
    pub secret_fields: BTreeSet<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub revision: u64,
    pub native_available: bool,
    pub modules: Vec<Connection>,
    pub builtin_azure_path: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub binary: Binary,
    pub manifest: Manifest,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Installation {
    pub expected_revision: u64,
    pub candidate: Candidate,
    pub confirmed: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub expected_revision: u64,
    pub values: BTreeMap<String, Value>,
    #[serde(default)]
    pub forget_values: BTreeSet<String>,
    pub secrets: BTreeMap<String, String>,
    pub forget_secrets: BTreeSet<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invocation {
    pub expected_revision: u64,
    pub method: Method,
    pub action: Option<String>,
    pub input: Value,
    pub confirmed: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteOperation {
    pub id: String,
    pub module: String,
    pub version: u64,
    pub revision: u64,
    pub manifest: Manifest,
    pub binary: Binary,
    pub values: BTreeMap<String, Value>,
    #[serde(default = "action_method")]
    pub method: Method,
    pub action: String,
    pub input: Value,
    pub state: crate::contracts::ForgeWriteState,
    pub started_at: String,
    pub updated_at: String,
    pub reply: Option<Reply>,
    pub request_id: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemotePreview {
    pub expected_revision: u64,
    #[serde(default = "action_method")]
    pub method: Method,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub input: Value,
}
fn action_method() -> Method {
    Method::Action
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketCreation {
    pub kind: String,
    pub title: String,
    pub description: String,
}
pub fn ticket_creation(input: &Value) -> Result<TicketCreation> {
    let input: TicketCreation =
        serde_json::from_value(input.clone()).map_err(|_| "message.invalid_ticket_creation")?;
    if !text(&input.kind, 100, false)
        || input
            .kind
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '$'))
        || !text(&input.title, 255, false)
        || !text(&input.description, 20_000, true)
    {
        return Err("message.invalid_ticket_type_title_or_description");
    }
    Ok(input)
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeTicket {
    pub connector: String,
    pub instance: String,
    pub id: String,
    pub key: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeSource {
    pub id: String,
    pub card_id: String,
    pub card_title: String,
    pub ticket: Option<TimeTicket>,
    pub seconds: u32,
    pub date: String,
    pub start_time: String,
    pub description: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeProposal {
    pub source: TimeSource,
    pub instance: String,
    pub account: String,
    pub reference: String,
}
pub fn time_proposal(value: &Value) -> Result<TimeProposal> {
    let proposal: TimeProposal =
        serde_json::from_value(value.clone()).map_err(|_| "message.invalid_time_export_preview")?;
    let source = &proposal.source;
    if !text(&source.id, 100, false)
        || !text(&source.card_id, 100, false)
        || !text(&source.card_title, 300, false)
        || !text(&source.description, 20_000, true)
        || !text(&proposal.instance, 2000, false)
        || !text(&proposal.account, 200, false)
        || !text(&proposal.reference, 2000, false)
        || source.ticket.as_ref().is_some_and(|ticket| {
            !text(&ticket.connector, 100, false)
                || !text(&ticket.instance, 2000, false)
                || !text(&ticket.id, 200, false)
                || !text(&ticket.key, 200, false)
        })
    {
        return Err("message.invalid_time_export_scope_or_entry");
    }
    let entry = serde_json::json!({"seconds":source.seconds,"date":source.date,"startTime":source.start_time,"description":source.description});
    let entry =
        serde_json::from_str(&entry.to_string()).map_err(|_| "message.invalid_time_entry")?;
    crate::domain::time::time_fields(&entry)?;
    Ok(proposal)
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteCommand {
    pub expected_revision: u64,
    pub expected_version: u64,
    pub confirmed: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteReceipt {
    pub operation_id: String,
    pub outcome: RemoteOutcome,
    pub reference: String,
    pub observed_at: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteOutcome {
    Applied,
    NotAttempted,
    Unknown,
}
pub fn remote_receipt(output: &Value, operation_id: &str) -> Result<Option<RemoteReceipt>> {
    let Some(value) = output.get("receipt") else {
        return Ok(None);
    };
    let receipt: RemoteReceipt = serde_json::from_value(value.clone())
        .map_err(|_| "message.invalid_connector_observation")?;
    if receipt.operation_id != operation_id
        || receipt.reference.trim().is_empty()
        || !text(&receipt.reference, 2000, false)
        || !text(&receipt.observed_at, 100, false)
    {
        return Err("message.the_observation_does_not_match_the_original_operation");
    }
    Ok(Some(receipt))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketIdentity {
    pub instance: String,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketTransition {
    pub id: String,
    pub name: String,
    pub status: String,
    pub requires_fields: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketTransitions {
    pub ticket: TicketIdentity,
    pub status: String,
    pub transitions: Vec<TicketTransition>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketTransitionPayload {
    pub card_id: String,
    pub card_title: String,
    pub card_revision: String,
    pub ticket: TicketIdentity,
    pub ticket_key: String,
    pub from_status: String,
    pub transition: TicketTransition,
}
pub fn ticket_transitions(value: &Value) -> Result<TicketTransitions> {
    let report: TicketTransitions =
        serde_json::from_value(value.clone()).map_err(|_| "message.invalid_module_transitions")?;
    if !text(&report.ticket.instance, 2000, false)
        || !text(&report.ticket.id, 200, false)
        || !text(&report.status, 200, false)
        || report.transitions.len() > 100
    {
        return Err("message.invalid_transition_status_or_scope");
    }
    let mut seen = BTreeSet::new();
    for transition in &report.transitions {
        if !text(&transition.id, 100, false)
            || transition.id.chars().any(char::is_control)
            || !text(&transition.name, 200, false)
            || !text(&transition.status, 200, false)
            || !seen.insert(&transition.id)
        {
            return Err("message.invalid_or_duplicate_module_transition");
        }
    }
    Ok(report)
}
pub fn ticket_transition_payload(value: &Value) -> Result<TicketTransitionPayload> {
    let payload: TicketTransitionPayload = serde_json::from_value(value.clone())
        .map_err(|_| "message.invalid_module_transition_preview")?;
    ticket_transitions(
        &serde_json::json!({"ticket":payload.ticket,"status":payload.from_status,"transitions":[payload.transition]}),
    )?;
    if !text(&payload.card_id, 100, false)
        || !text(&payload.card_title, 300, false)
        || !text(&payload.ticket_key, 100, false)
        || payload.card_revision.len() != 64
        || !payload
            .card_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || payload.transition.requires_fields
    {
        return Err("message.the_transition_requires_additional_fields_or_a_new_task_snapshot");
    }
    Ok(payload)
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ticket {
    pub instance: String,
    pub id: String,
    pub key: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub status: String,
}
impl Ticket {
    pub fn identity(&self) -> TicketIdentity {
        TicketIdentity {
            instance: self.instance.clone(),
            id: self.id.clone(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketPage {
    pub items: Vec<Ticket>,
    pub next: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum TicketSync {
    Ready { page: TicketPage },
    ConfigurationRequired,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketImport {
    pub expected_revision: u64,
    pub report_id: String,
    pub project: crate::document::Document,
    pub tickets: Vec<TicketIdentity>,
    pub confirmed: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedTicket {
    pub card: crate::contracts::Card,
    pub created: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketRefresh {
    pub expected_revision: u64,
    pub report_id: String,
    pub expected: crate::contracts::Card,
    pub replace_title: bool,
    pub replace_description: bool,
    pub confirmed: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportItem {
    pub label: String,
    pub value: String,
}
pub fn report_items(report: &Report) -> Result<Vec<ReportItem>> {
    if !matches!(
        report.method,
        Method::Check | Method::Action | Method::TicketsCreate
    ) {
        return Ok(Vec::new());
    }
    let Some(items) = report.output.get("items") else {
        return Ok(Vec::new());
    };
    let items = serde_json::from_value::<Vec<ReportItem>>(items.clone())
        .map_err(|_| "message.invalid_connector_report_items")?;
    if items.len() > 100
        || items
            .iter()
            .any(|item| !text(&item.label, 200, false) || !text(&item.value, 2000, true))
    {
        return Err("message.connector_report_items_exceed_the_limit");
    }
    Ok(items)
}

pub fn ticket_page(method: Method, output: &Value) -> Result<TicketPage> {
    let page = match method {
        Method::TicketsSync => match serde_json::from_value::<TicketSync>(output.clone())
            .map_err(|_| "message.invalid_connector_ticket_page")?
        {
            TicketSync::Ready { page } => page,
            TicketSync::ConfigurationRequired => {
                return Err("message.ticket_sync_configuration_required");
            }
        },
        Method::TicketsSearch => serde_json::from_value::<TicketPage>(output.clone())
            .map_err(|_| "message.invalid_connector_ticket_page")?,
        Method::TicketsRead => TicketPage {
            items: vec![
                serde_json::from_value::<Ticket>(output.clone())
                    .map_err(|_| "message.invalid_connector_ticket")?,
            ],
            next: None,
        },
        _ => return Err("message.this_report_contains_no_tickets"),
    };
    if page.items.len() > 100
        || page
            .next
            .as_ref()
            .is_some_and(|next| !text(next, 2000, false))
    {
        return Err("message.connector_ticket_page_exceeds_the_limit");
    }
    let mut identities = BTreeSet::new();
    for ticket in &page.items {
        if !text(&ticket.instance, 2000, false)
            || !text(&ticket.id, 200, false)
            || !text(&ticket.key, 100, false)
            || !text(&ticket.title, 300, false)
            || ticket.title.trim().is_empty()
            || !text(&ticket.description, 20_000, true)
            || !text(&ticket.url, 2000, false)
            || !text(&ticket.status, 200, true)
            || !identities.insert(ticket.identity())
        {
            return Err("message.invalid_or_duplicate_connector_ticket");
        }
    }
    Ok(page)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Describe,
    Check,
    TicketsSearch,
    TicketsSync,
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
        return Err("message.connector_manifest_invalid");
    }
    let methods = manifest.methods.iter().copied().collect::<BTreeSet<_>>();
    if methods.contains(&Method::RepositoriesPublish)
        && ![
            Method::RepositoriesPreview,
            Method::RepositoriesRefresh,
            Method::RepositoriesReview,
        ]
        .iter()
        .all(|method| methods.contains(method))
    {
        return Err("message.repository_publication_methods_required");
    }
    if methods.len() != manifest.methods.len()
        || !methods.contains(&Method::Check)
        || methods.contains(&Method::Describe)
    {
        return Err("message.connector_methods_invalid");
    }
    if methods.iter().any(|method| {
        method.effect() == Effect::Remote
            && !methods.contains(&if *method == Method::TimeSend {
                Method::TimeReconcile
            } else {
                Method::Reconcile
            })
    }) {
        return Err("message.connector_reconciliation_required");
    }
    if methods.contains(&Method::TimeSend) && !methods.contains(&Method::TimePreview) {
        return Err("message.a_time_export_requires_a_destination_preview_before_sending");
    }
    if methods.contains(&Method::TicketsTransition)
        && !methods.contains(&Method::TicketsTransitions)
    {
        return Err("message.a_ticket_transition_requires_reading_the_available_choices_first");
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
            return Err("message.connector_field_invalid");
        }
        let mut values = BTreeSet::new();
        for option in &field.options {
            if !text(&option.value, 4000, true)
                || !text(&option.label, 200, false)
                || !values.insert(&option.value)
            {
                return Err("message.connector_choice_invalid");
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
            return Err("message.connector_tab_invalid");
        }
        for action in &tab.actions {
            if !identifier(&action.id)
                || !actions.insert(&action.id)
                || !text(&action.label, 200, false)
                || (action.effect == Effect::Remote && !methods.contains(&Method::Reconcile))
            {
                return Err("message.connector_action_invalid");
            }
        }
    }
    if methods.contains(&Method::Action) {
        if actions.is_empty() {
            return Err("message.connector_actions_inconsistent");
        }
    } else if !actions.is_empty() {
        return Err("message.connector_actions_inconsistent");
    }
    if manifest.translations.len() > 2 {
        return Err("message.connector_catalog_too_large");
    }
    for (locale, messages) in &manifest.translations {
        if !["fr-FR", "en-US"].contains(&locale.as_str())
            || messages.len() > 200
            || messages
                .iter()
                .any(|(key, value)| !identifier(key) || !text(value, 4000, true))
        {
            return Err("message.connector_catalog_invalid");
        }
        if messages.keys().any(|key| {
            !manifest
                .translations
                .get(&manifest.source_locale)
                .is_some_and(|source| source.contains_key(key))
        }) {
            return Err("message.connector_catalog_invalid");
        }
    }
    if serde_json::to_vec(manifest)
        .map_err(|_| "message.connector_manifest_invalid")?
        .len()
        > 128 * 1024
    {
        return Err("message.connector_manifest_too_large");
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
        return Err("message.invalid_connector_request_identity");
    }
    if request.method == Method::Describe {
        if request.action.is_some()
            || request.operation_id.is_some()
            || !request.settings.is_empty()
            || !request.secrets.is_empty()
            || request.input != Value::Null
        {
            return Err("message.connector_discovery_context_forbidden");
        }
        return Ok(Effect::Read);
    }
    if !manifest.methods.contains(&request.method) {
        return Err("message.connector_method_unsupported");
    }
    if serde_json::to_vec(request)
        .map_err(|_| "message.connector_request_invalid")?
        .len()
        > INPUT_LIMIT
    {
        return Err("message.connector_request_too_large");
    }
    for (key, value) in &request.settings {
        let field = manifest
            .fields
            .iter()
            .find(|field| &field.id == key && field.kind != FieldKind::Password)
            .ok_or("message.connector_public_setting_unknown")?;
        if field.kind == FieldKind::Checkbox {
            if !value.is_boolean() {
                return Err("message.connector_value_invalid");
            }
        } else {
            let value = value
                .as_str()
                .filter(|value| text(value, 4000, true))
                .ok_or("message.connector_value_invalid")?;
            if field.kind == FieldKind::Select
                && !field.options.iter().any(|option| option.value == value)
            {
                return Err("message.connector_choice_invalid");
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
            return Err("message.connector_secret_invalid");
        }
    }
    let effect = if request.method == Method::Action {
        manifest
            .tabs
            .iter()
            .flat_map(|tab| &tab.actions)
            .find(|action| Some(&action.id) == request.action.as_ref())
            .map(|action| action.effect)
            .ok_or("message.connector_action_unknown")?
    } else {
        if request.action.is_some() {
            return Err("message.connector_action_unexpected");
        }
        request.method.effect()
    };
    if effect != Effect::Read && request.operation_id.is_none() {
        return Err("message.connector_write_requires_intent");
    }
    if matches!(request.method, Method::Reconcile | Method::TimeReconcile)
        && request.operation_id.is_none()
    {
        return Err("message.connector_reconcile_requires_operation");
    }
    Ok(effect)
}

pub fn decode_reply(bytes: &[u8], request: &Request) -> Result<Reply> {
    if bytes.len() > OUTPUT_LIMIT {
        return Err("message.connector_reply_too_large");
    }
    let reply: Reply =
        serde_json::from_slice(bytes).map_err(|_| "message.connector_reply_invalid")?;
    if reply.protocol != PROTOCOL
        || reply.request_id != request.request_id
        || reply.module != request.module
        || reply.method != request.method
        || !text(&reply.message, 2000, true)
    {
        return Err("message.connector_reply_mismatch");
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    #[test]
    fn synchronization_declares_read_only_pages_or_missing_configuration() {
        use super::*;
        assert_eq!(Method::TicketsSync.effect(), Effect::Read);
        let missing = serde_json::json!({"state":"configurationRequired"});
        assert!(matches!(
            serde_json::from_value::<TicketSync>(missing.clone()).unwrap(),
            TicketSync::ConfigurationRequired
        ));
        assert!(ticket_page(Method::TicketsSync, &missing).is_err());
        let ready = serde_json::json!({"state":"ready","page":{"items":[],"next":"page-2"}});
        assert_eq!(
            ticket_page(Method::TicketsSync, &ready)
                .unwrap()
                .next
                .as_deref(),
            Some("page-2")
        );
        assert!(
            ticket_page(
                Method::TicketsSync,
                &serde_json::json!({"state":"ready","page":{"items":[],"next":""}})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<TicketSync>(
                serde_json::json!({"state":"configurationRequired","items":[]})
            )
            .is_err()
        );
    }

    use super::*;
    #[test]
    fn structured_report_items_have_explicit_text_and_count_bounds() {
        let mut report = Report {
            id: "synthetic".into(),
            method: Method::Action,
            action: Some("count".into()),
            message: "Synthetic".into(),
            observed_at: "2026-10-25T00:00:00Z".into(),
            output: serde_json::json!({"items":[{"label":"Imported tickets","value":"3"}]}),
        };
        assert_eq!(report_items(&report).unwrap()[0].value, "3");
        report.output["items"][0]["value"] = serde_json::json!(3);
        assert!(report_items(&report).is_err());
        report.output = serde_json::json!({"items":vec![serde_json::json!({"label":"Repeated","value":"0"});101]});
        assert!(report_items(&report).is_err());
        report.output = serde_json::json!({"items":[{"label":"Bounded","value":"x".repeat(2001)}]});
        assert!(report_items(&report).is_err());
    }
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
    #[test]
    fn connector_translations_use_keys_with_a_source_message() {
        let mut candidate = manifest();
        candidate.translations.insert(
            "en-US".into(),
            BTreeMap::from([("connector-name".into(), "Connector".into())]),
        );
        assert!(validate_manifest(&candidate).is_err());
        candidate.translations.insert(
            "fr-FR".into(),
            BTreeMap::from([("connector-name".into(), "Connecteur".into())]),
        );
        assert!(validate_manifest(&candidate).is_ok());
        candidate
            .translations
            .get_mut("fr-FR")
            .unwrap()
            .insert("Nom du connecteur".into(), "Connecteur".into());
        assert!(validate_manifest(&candidate).is_err());
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
        bad.methods
            .extend([Method::TimePreview, Method::TimeReconcile]);
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
            .extend([Method::TimePreview, Method::TimeSend, Method::TimeReconcile]);
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
