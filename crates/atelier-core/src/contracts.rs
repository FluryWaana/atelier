use crate::document::{Document, JsonString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct RunLog {
    pub seq: i64,
    pub stream: String,
    pub text: String,
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
