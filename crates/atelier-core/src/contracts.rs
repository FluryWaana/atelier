use crate::document::{Document, JsonString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct RunLog {
    pub seq: i64,
    pub stream: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub multi_select: bool,
    pub is_secret: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInteraction {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub can_allow: bool,
    pub questions: Vec<AgentQuestion>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAnswer {
    pub request_id: String,
    pub decision: String,
    #[serde(default)]
    pub answers: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JiraMode {
    Cloud,
    DataCenter,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraConfiguration {
    pub mode: JiraMode,
    pub url: String,
    pub email: String,
    pub query: String,
    pub revision: u64,
}

impl Default for JiraConfiguration {
    fn default() -> Self {
        Self {
            mode: JiraMode::Cloud,
            url: String::new(),
            email: String::new(),
            query: "assignee = currentUser() AND sprint in openSprints() ORDER BY updated DESC"
                .into(),
            revision: 0,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraConnection {
    pub configuration: JiraConfiguration,
    pub token_set: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraConfigurationUpdate {
    pub expected_revision: u64,
    pub mode: JiraMode,
    pub url: String,
    pub email: String,
    pub query: String,
    pub token: Option<String>,
    #[serde(default)]
    pub clear_token: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct JiraIssue {
    pub id: String,
    pub key: String,
    pub title: String,
    pub status: String,
    pub url: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueDetail {
    pub connection_revision: u64,
    pub issue: JiraIssue,
    pub description: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSearch {
    pub connection_revision: u64,
    pub issues: Vec<JiraIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAuthentication {
    Unavailable,
    Present,
    Absent,
    Configured,
    MissingKey,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDiagnostic {
    pub executable: String,
    pub version: Option<String>,
    pub version_error: Option<String>,
    pub authentication: AgentAuthentication,
    pub authentication_error: Option<String>,
    pub account: Option<String>,
    pub auth_method: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServiceEndpoint {
    pub pid: u32,
    pub address: String,
    pub port: u16,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub exit_code: Option<i32>,
    pub log: String,
    pub error: String,
    pub process_ids: Vec<u32>,
    pub endpoints: Vec<ServiceEndpoint>,
    pub inspection_error: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSession {
    pub card_id: String,
    pub project_ids: Vec<String>,
    pub services: Vec<ServiceState>,
}

#[derive(Clone, Debug)]
pub struct Card {
    pub id: String,
    pub title: JsonString,
    pub fields: Document,
}

impl Serialize for Card {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut document = self.fields.clone();
        document
            .set("id", &self.id)
            .map_err(serde::ser::Error::custom)?;
        document
            .set("title", &self.title)
            .map_err(serde::ser::Error::custom)?;
        document.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Card {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut document = Document::deserialize(deserializer)?;
        let id = document
            .get::<String>("id")
            .map_err(serde::de::Error::custom)?
            .ok_or_else(|| serde::de::Error::missing_field("id"))?;
        let title = document
            .get::<JsonString>("title")
            .map_err(serde::de::Error::custom)?
            .ok_or_else(|| serde::de::Error::missing_field("title"))?;
        document.remove("id");
        document.remove("title");
        Ok(Card {
            id,
            title,
            fields: document,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Board {
    /// Monotonic within one server session; a new local session starts a new sequence.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub settings: Document,
    pub cards: Vec<Card>,
    #[serde(default)]
    pub brainstorms: Vec<Card>,
    #[serde(default)]
    pub projects: Vec<Document>,
    #[serde(default)]
    pub runs: Vec<Document>,
    #[serde(default)]
    pub times: Vec<Document>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileListing {
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FilePreview {
    pub path: String,
    pub content: Option<String>,
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub untracked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeListing {
    pub base: String,
    pub files: Vec<FileChange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub patch: Option<String>,
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub source: String,
    pub branch: String,
    pub base: String,
    pub base_commit: String,
    pub state: WorktreeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeState {
    Creating,
    Ready,
    Removing,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraImport {
    pub revision: u64,
    pub issue_id: String,
    pub project_id: String,
    #[serde(default)]
    pub attach_to: Option<Card>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct JiraImported {
    pub card: Card,
    pub created: bool,
    pub attached: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct JiraStatus {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraTransition {
    pub id: String,
    pub name: String,
    pub target: JiraStatus,
    pub requires_fields: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraTransitions {
    pub connection_revision: u64,
    pub current: JiraStatus,
    pub transitions: Vec<JiraTransition>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraTransitionRequest {
    pub revision: u64,
    pub expected: Card,
    pub from: JiraStatus,
    pub transition: JiraTransition,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraReconciliation {
    pub revision: u64,
    pub expected: Card,
    #[serde(default)]
    pub dismiss: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JiraTransitionState {
    Preparing,
    Sending,
    Uncertain,
    Confirmed,
    Failed,
    Dismissed,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraTransitionIntent {
    pub id: String,
    pub state: JiraTransitionState,
    pub base: String,
    pub issue_id: String,
    pub revision: u64,
    pub from: JiraStatus,
    pub transition: JiraTransition,
    pub started_at: String,
    pub observed: Option<JiraStatus>,
}
