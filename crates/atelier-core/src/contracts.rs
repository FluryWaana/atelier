use crate::document::{Document, JsonString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TempoConfiguration {
    pub base: String,
    pub revision: u64,
}
impl Default for TempoConfiguration {
    fn default() -> Self {
        Self {
            base: "https://api.tempo.io/4".into(),
            revision: 0,
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoConnection {
    pub configuration: TempoConfiguration,
    pub token_set: bool,
    pub jira_url: String,
    pub jira_revision: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TempoConfigurationUpdate {
    pub expected_revision: u64,
    pub expected_jira_revision: u64,
    pub base: String,
    pub token: Option<String>,
    pub clear_token: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TempoPayload {
    pub issue_id: u64,
    pub author_account_id: String,
    pub time_spent_seconds: u32,
    pub start_date: String,
    pub start_time: String,
    pub description: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoExportPreview {
    pub entry: Document,
    pub card_id: String,
    pub ticket: Document,
    pub jira_url: String,
    pub jira_revision: u64,
    pub tempo: TempoConfiguration,
    pub payload: TempoPayload,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoExportIntent {
    pub id: String,
    pub state: ForgeWriteState,
    pub preview: TempoExportPreview,
    pub worklog_id: Option<u64>,
    pub observed_at: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TempoExportRequest {
    pub expected: Document,
    pub expected_card: Card,
    pub jira_revision: u64,
    pub tempo_revision: u64,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalCommitSnapshot {
    pub branch: String,
    pub head: String,
    pub tree: String,
    pub paths: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalCommitRequest {
    pub expected: Card,
    pub paths: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub message: String,
    pub confirmed: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCommitPreview {
    pub worktree: Worktree,
    pub snapshot: LocalCommitSnapshot,
    pub author_name: String,
    pub author_email: String,
    pub message: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCommitIntent {
    pub id: String,
    pub state: ForgeWriteState,
    pub preview: LocalCommitPreview,
    pub commit: Option<String>,
    pub observed: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCommitResponse {
    pub card: Card,
    pub patch: String,
}

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
    #[serde(default)]
    pub status_id: String,
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
pub enum JiraWriteState {
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
    pub state: JiraWriteState,
    pub base: String,
    pub issue_id: String,
    pub revision: u64,
    pub from: JiraStatus,
    pub transition: JiraTransition,
    pub started_at: String,
    pub observed: Option<JiraStatus>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JiraRefreshState {
    Reading,
    Ready,
    Interrupted,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraRefreshPreview {
    pub id: String,
    pub state: JiraRefreshState,
    pub revision: u64,
    pub base: String,
    pub issue_id: String,
    pub expected: Card,
    pub detail: Option<JiraIssueDetail>,
    pub created_at: String,
    #[serde(default)]
    pub mapping: serde_json::Value,
    pub mapped_status: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraRefreshRequest {
    pub revision: u64,
    pub expected: Card,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraRefreshApply {
    pub revision: u64,
    pub expected: Card,
    pub preview_id: String,
    #[serde(default)]
    pub apply_status: bool,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JiraIssueType {
    pub id: String,
    pub name: String,
    pub subtask: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCreationField {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub has_default: bool,
    pub can_set: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCreationMetadata {
    pub revision: u64,
    pub project_key: String,
    pub issue_type: JiraIssueType,
    pub fields: Vec<JiraCreationField>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraCreateRequest {
    pub revision: u64,
    pub expected: Card,
    pub project_key: String,
    pub issue_type: JiraIssueType,
    pub confirmed: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraCreationResolve {
    pub revision: u64,
    pub expected: Card,
    pub existing_key: Option<String>,
    #[serde(default)]
    pub close_uncertainty: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCreationIntent {
    pub id: String,
    pub state: JiraWriteState,
    pub revision: u64,
    pub base: String,
    pub project_key: String,
    pub issue_type: JiraIssueType,
    pub started_at: String,
    pub result: Option<JiraIssue>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraBatchImport {
    pub revision: u64,
    pub issue_ids: Vec<String>,
    pub project_id: String,
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    Github,
    Gitlab,
    Azure,
}
impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Azure => "azure",
        }
    }
}
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeConfiguration {
    pub kind: ForgeKind,
    pub base: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeConnection {
    pub configuration: ForgeConfiguration,
    pub token_set: bool,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct ForgeConnections {
    pub revision: u64,
    pub connections: Vec<ForgeConnection>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForgeUpdate {
    pub expected_revision: u64,
    pub configuration: ForgeConfiguration,
    pub token: Option<String>,
    #[serde(default)]
    pub clear_token: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub revision: u64,
    pub kind: ForgeKind,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub status: String,
    pub draft: bool,
    pub source_branch: String,
    pub target_branch: String,
    pub source_commit: Option<String>,
    pub target_commit: Option<String>,
    pub fork: bool,
    pub base_url: String,
    pub checked_at: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSummary {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub draft: bool,
    pub source_branch: String,
    pub target_branch: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunQueue {
    pub revision: u64,
    pub ids: Vec<String>,
    pub paused: bool,
    pub active: Option<QueuedRun>,
    pub message: String,
    #[serde(default)]
    pub workflows: std::collections::BTreeMap<String, Option<String>>,
}
impl Default for RunQueue {
    fn default() -> Self {
        Self {
            revision: 0,
            ids: Vec::new(),
            paused: true,
            active: None,
            message: String::new(),
            workflows: std::collections::BTreeMap::new(),
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedRun {
    pub card_id: String,
    pub run_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestAttach {
    pub expected: Card,
    pub revision: u64,
    pub kind: ForgeKind,
    pub repository: String,
    pub number: u64,
    pub confirmed: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequestDetach {
    pub expected: Card,
    pub confirmed: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForgeDraftRequest {
    pub expected: Card,
    pub revision: u64,
    pub draft: bool,
    pub confirmed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ForgeWriteState {
    Preparing,
    Sending,
    Uncertain,
    Confirmed,
    Failed,
    Reconciled,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeDraftIntent {
    pub id: String,
    pub state: ForgeWriteState,
    pub expected: PullRequest,
    pub draft: bool,
    pub started_at: String,
    pub observed: Option<PullRequest>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitPushRequest {
    pub expected: Card,
    pub revision: u64,
    pub confirmed: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPushPreview {
    pub configuration: ForgeConfiguration,
    pub revision: u64,
    pub repository: String,
    pub worktree: Worktree,
    pub pull_request: Option<PullRequest>,
    pub origin: String,
    pub commit: String,
    pub previous: Option<String>,
    pub local_changes: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPushIntent {
    pub id: String,
    pub state: ForgeWriteState,
    pub preview: GitPushPreview,
    pub observed: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForgePublicationRequest {
    pub expected: Card,
    pub revision: u64,
    pub kind: ForgeKind,
    pub repository: String,
    pub target: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
    pub confirmed: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgePublicationPreview {
    pub configuration: ForgeConfiguration,
    pub revision: u64,
    pub repository: String,
    pub worktree: Worktree,
    pub target: String,
    pub origin: String,
    pub remote_source: Option<String>,
    pub local_changes: bool,
    pub source_commit: String,
    pub target_commit: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgePublicationIntent {
    pub id: String,
    pub state: ForgeWriteState,
    pub preview: ForgePublicationPreview,
    pub number: Option<u64>,
    pub result: Option<PullRequest>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ForgeCheckState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ForgeCheckScope {
    SourceCommit,
    OtherSnapshot,
    Unscoped,
}
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeCheck {
    pub id: String,
    pub name: String,
    pub state: ForgeCheckState,
    pub scope: ForgeCheckScope,
    pub commit: Option<String>,
    pub description: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ForgeReviewDecision {
    Approved,
    ApprovedWithSuggestions,
    ChangesRequested,
    Waiting,
    Dismissed,
    None,
    Unknown,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeReviewer {
    pub name: String,
    pub decision: ForgeReviewDecision,
    pub scope: ForgeCheckScope,
    pub commit: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeChecks {
    pub pull_request: PullRequest,
    pub checks: Vec<ForgeCheck>,
    pub reviewers: Vec<ForgeReviewer>,
    pub reviewers_available: bool,
    pub other_snapshot_count: usize,
    pub limits: Vec<String>,
}
