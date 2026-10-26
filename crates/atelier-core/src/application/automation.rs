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
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Parcours en cours",
            Self::Waiting => "Publication à confirmer",
            Self::Ready => "Prêt pour le contrôle humain",
            Self::Blocked => "Parcours bloqué",
            Self::Interrupted => "Parcours interrompu",
            Self::Error => "Parcours en échec",
            Self::Cancelled => "Parcours arrêté",
        }
    }
}
impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Self::Development => "Développement",
            Self::Tests => "Vérification",
            Self::Commit => "Commit",
            Self::Publication => "Publication",
            Self::Review => "Review",
            Self::Correction => "Correction",
        }
    }
}
