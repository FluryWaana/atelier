use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
};
use serde_json::value::RawValue;
use std::fmt;

/// Open JSON fields are retained for extensions and forward-compatible metadata.
#[derive(Clone, Debug, Default)]
pub struct Document(Vec<(String, Box<RawValue>)>);

impl Document {
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(key, _)| key.as_str())
    }

    pub fn raw(&self, key: &str) -> Option<&RawValue> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, raw)| raw.as_ref())
    }

    pub fn flag(&self, key: &str) -> serde_json::Result<bool> {
        Ok(self.get::<Option<bool>>(key)?.flatten().unwrap_or(false))
    }

    pub fn has_text(&self, key: &str) -> serde_json::Result<bool> {
        Ok(self
            .get::<Option<String>>(key)?
            .flatten()
            .is_some_and(|value| !value.is_empty()))
    }

    pub fn has_object(&self, key: &str) -> serde_json::Result<bool> {
        Ok(self.get::<Option<Document>>(key)?.flatten().is_some())
    }

    pub fn has_list(&self, key: &str) -> serde_json::Result<bool> {
        Ok(self.get::<Option<Vec<Document>>>(key)?.flatten().is_some())
    }

    pub fn string_equals(&self, key: &str, expected: &str) -> serde_json::Result<bool> {
        Ok(self
            .get::<String>(key)?
            .is_some_and(|value| value == expected))
    }

    pub fn get<T: de::DeserializeOwned>(&self, key: &str) -> serde_json::Result<Option<T>> {
        self.raw(key)
            .map(|raw| serde_json::from_str(raw.get()))
            .transpose()
    }

    pub fn set<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> serde_json::Result<()> {
        self.set_key(key.to_owned(), value)
    }

    pub fn set_key<T: Serialize + ?Sized>(
        &mut self,
        key: String,
        value: &T,
    ) -> serde_json::Result<()> {
        let raw = serde_json::value::to_raw_value(value)?;
        serde_json::from_str::<serde_json::Value>(raw.get())?;
        self.insert(key, raw);
        Ok(())
    }

    fn insert(&mut self, key: String, raw: Box<RawValue>) {
        if let Some((_, previous)) = self.0.iter_mut().find(|(name, _)| name == &key) {
            *previous = raw;
        } else {
            self.0.push((key, raw));
        }
    }

    pub fn overlay(&mut self, other: &Self) {
        for (key, raw) in &other.0 {
            self.insert(key.clone(), raw.clone());
        }
    }

    pub fn from_raw(raw: &RawValue) -> serde_json::Result<Self> {
        serde_json::from_str(raw.get())
    }

    pub fn remove(&mut self, key: &str) {
        self.0.retain(|(name, _)| name != key);
    }
}

impl Serialize for Document {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, raw) in &self.0 {
            map.serialize_entry(key, raw)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Document;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object with valid Unicode text")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Document, M::Error> {
                let mut document = Document::default();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    serde_json::from_str::<serde_json::Value>(value.get())
                        .map_err(de::Error::custom)?;
                    document.insert(key, value);
                }
                Ok(document)
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_and_optional_fields_never_coerce_other_json_types() {
        let mut document = Document::default();
        assert!(!document.flag("archived").unwrap());
        document.set("archived", &false).unwrap();
        assert!(!document.flag("archived").unwrap());
        for raw in ["1", "0", "\"false\"", "[]", "{}"] {
            document
                .set("archived", &RawValue::from_string(raw.into()).unwrap())
                .unwrap();
            assert!(document.flag("archived").is_err(), "{raw}");
        }
        document.set("repo", &false).unwrap();
        assert!(document.has_text("repo").is_err());
        document.set("ticket", &"SYN-1").unwrap();
        assert!(document.has_object("ticket").is_err());
        document.set("checklist", &serde_json::json!({})).unwrap();
        assert!(document.has_list("checklist").is_err());
    }

    #[test]
    fn edits_preserve_open_fields_and_numeric_representation() {
        let mut document: Document = serde_json::from_str(
            r#"{"title":"Tâche 🔧","extra":{"é":[true,null]},"number":1.2300}"#,
        )
        .unwrap();
        document.set("title", &"Nouvelle tâche 🦀").unwrap();
        document.set("archived", &false).unwrap();
        let encoded = serde_json::to_string(&document).unwrap();
        assert!(encoded.contains("1.2300"));
        let reopened: Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            reopened.get::<String>("title").unwrap().as_deref(),
            Some("Nouvelle tâche 🦀")
        );
        assert_eq!(
            reopened.raw("extra").unwrap().get(),
            document.raw("extra").unwrap().get()
        );
    }

    #[test]
    fn malformed_unicode_is_rejected_at_every_document_boundary() {
        for raw in [
            r#"{"title":"\ud800"}"#,
            r#"{"\udc00":true}"#,
            r#"{"future":{"nested":["\ud83d"]}}"#,
        ] {
            assert!(serde_json::from_str::<Document>(raw).is_err());
        }
        let raw = RawValue::from_string(r#""\ud800""#.into()).unwrap();
        let mut document = Document::default();
        assert!(document.set("title", &raw).is_err());
        assert!(document.raw("title").is_none());
        for raw in ["[]", "null", "true", "1", "\"text\""] {
            assert!(Document::from_raw(&RawValue::from_string(raw.into()).unwrap()).is_err());
        }
    }
}
