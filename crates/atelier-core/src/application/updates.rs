use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Prepared,
    Waiting,
    Activating,
    Trying,
    Committed,
    RollingBack,
    RolledBack,
    Cancelled,
    Failed,
    Uncertain,
}
impl Phase {
    pub fn maintenance(self) -> bool {
        matches!(
            self,
            Self::Waiting | Self::Activating | Self::Trying | Self::RollingBack | Self::Uncertain
        )
    }
    pub fn pending(self) -> bool {
        matches!(self, Self::Preparing | Self::Prepared) || self.maintenance()
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub size: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub state: Phase,
    pub from_version: String,
    pub to_version: String,
    pub key_id: String,
    pub sha256: String,
    pub created_at: String,
    pub message: String,
    pub files: Vec<File>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    pub installed: bool,
    pub current_version: String,
    pub target: String,
    pub operation: Option<Operation>,
}
