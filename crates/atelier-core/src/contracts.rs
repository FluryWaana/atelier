use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Board {
    pub cards: Vec<Card>,
}
