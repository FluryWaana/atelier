use crate::document::{Document, JsonString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
