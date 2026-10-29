use crate::{contracts::Worktree, document::Document};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Running,
    Waiting,
    Ready,
    Blocked,
    Interrupted,
    Error,
    Cancelled,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Development,
    Tests,
    Commit,
    Publication,
    Review,
    Correction,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "operation", content = "preview", rename_all = "camelCase")]
pub enum PublicationPlan {
    Create(Box<crate::contracts::ForgePublicationPreview>),
    Push(Box<crate::contracts::GitPushPreview>),
    Module(Box<super::extensions::RepositoryProposal>),
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub state: State,
    pub step: Step,
    pub project: Document,
    pub requirements: serde_json::Value,
    pub worktree: Worktree,
    pub agent: String,
    pub profile: Document,
    pub model: String,
    pub effort: String,
    pub instructions: String,
    pub run_id: Option<String>,
    pub review_id: Option<String>,
    pub verification_id: Option<String>,
    pub publication_plan: Option<PublicationPlan>,
    pub commit_plan: Option<Box<crate::contracts::LocalCommitPreview>>,
    pub review_requested: bool,
    pub correction_indexes: Vec<usize>,
    pub cycles: u8,
    pub auto_fix: bool,
    pub started_at: String,
    pub updated_at: String,
    pub message: String,
}

impl State {
    pub fn card_status(self) -> &'static str {
        match self {
            Self::Running => "doing",
            Self::Ready => "review",
            _ => "standby",
        }
    }
    pub fn translation_key(self) -> &'static str {
        match self {
            Self::Running => "message.workflow_running",
            Self::Waiting => "message.publication_awaiting_confirmation",
            Self::Ready => "message.ready_for_human_review",
            Self::Blocked => "message.workflow_blocked",
            Self::Interrupted => "message.workflow_interrupted",
            Self::Error => "message.workflow_failed",
            Self::Cancelled => "message.workflow_stopped",
        }
    }
}
impl Step {
    pub fn translation_key(self) -> &'static str {
        match self {
            Self::Development => "message.development",
            Self::Tests => "message.verification",
            Self::Commit => "message.commit",
            Self::Publication => "message.publication",
            Self::Review => "message.review",
            Self::Correction => "message.correction",
        }
    }
}
