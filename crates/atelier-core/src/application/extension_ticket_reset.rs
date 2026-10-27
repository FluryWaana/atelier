use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketResetRead {
    pub expected_revision: u64,
    pub confirmed: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketResetRow {
    pub card_id: String,
    pub title: String,
    pub key: String,
    pub present: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketResetPreview {
    pub id: String,
    pub module: String,
    pub revision: u64,
    pub observed_at: String,
    pub pages: usize,
    pub tickets: usize,
    pub rows: Vec<TicketResetRow>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketResetApply {
    pub preview_id: String,
    pub expected_revision: u64,
    pub confirmed: bool,
}
