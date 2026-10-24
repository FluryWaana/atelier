use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;

/// JSON strings use UTF-16 units, including isolated surrogates produced by Node's slice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JsonString(pub Vec<u16>);

impl From<&str> for JsonString {
    fn from(value: &str) -> Self {
        Self(value.encode_utf16().collect())
    }
}

impl JsonString {
    pub fn utf8(&self) -> Result<String, std::string::FromUtf16Error> {
        String::from_utf16(&self.0)
    }

    fn escaped(&self) -> String {
        use std::fmt::Write;
        let mut result = String::from("\"");
        for character in char::decode_utf16(self.0.iter().copied()) {
            match character {
                Ok('"') => result.push_str("\\\""),
                Ok('\\') => result.push_str("\\\\"),
                Ok(character) if character <= '\u{1f}' => {
                    write!(result, "\\u{:04x}", u32::from(character))
                        .expect("String writes cannot fail");
                }
                Ok(character) => result.push(character),
                Err(surrogate) => {
                    write!(result, "\\u{:04x}", surrogate.unpaired_surrogate())
                        .expect("String writes cannot fail");
                }
            }
        }
        result.push('"');
        result
    }
}

impl Serialize for JsonString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawValue::from_string(self.escaped())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StringVisitor;
        impl Visitor<'_> for StringVisitor {
            type Value = JsonString;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON string")
            }
            fn visit_bytes<E: de::Error>(self, mut bytes: &[u8]) -> Result<JsonString, E> {
                let mut units = Vec::new();
                while !bytes.is_empty() {
                    match std::str::from_utf8(bytes) {
                        Ok(value) => {
                            units.extend(value.encode_utf16());
                            break;
                        }
                        Err(error) => {
                            let prefix = error.valid_up_to();
                            units.extend(
                                std::str::from_utf8(&bytes[..prefix])
                                    .map_err(E::custom)?
                                    .encode_utf16(),
                            );
                            bytes = &bytes[prefix..];
                            // serde_json::deserialize_bytes represents lone UTF-16 surrogates as WTF-8.
                            if bytes.len() < 3
                                || bytes[0] != 0xed
                                || !(0xa0..=0xbf).contains(&bytes[1])
                                || !(0x80..=0xbf).contains(&bytes[2])
                            {
                                return Err(E::custom("Invalid WTF-8 JSON string"));
                            }
                            units.push(
                                0xd000
                                    | (u16::from(bytes[1] & 0x3f) << 6)
                                    | u16::from(bytes[2] & 0x3f),
                            );
                            bytes = &bytes[3..];
                        }
                    }
                }
                Ok(JsonString(units))
            }
        }
        deserializer.deserialize_bytes(StringVisitor)
    }
}

/// Unknown values remain raw JSON; changing a known field cannot normalize them.
#[derive(Clone, Debug, Default)]
pub struct Document(Vec<(JsonString, Box<RawValue>)>);

impl Document {
    pub fn keys(&self) -> impl Iterator<Item = &JsonString> {
        self.0.iter().map(|(key, _)| key)
    }
    pub fn raw(&self, key: &str) -> Option<&RawValue> {
        self.raw_key(&JsonString::from(key))
    }

    pub fn raw_key(&self, key: &JsonString) -> Option<&RawValue> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, raw)| raw.as_ref())
    }

    pub fn truthy(&self, key: &str) -> serde_json::Result<bool> {
        let Some(raw) = self.raw(key) else {
            return Ok(false);
        };
        match raw.get().as_bytes().first() {
            Some(b'{' | b'[') => Ok(true),
            Some(b'"') => Ok(!serde_json::from_str::<JsonString>(raw.get())?.0.is_empty()),
            _ => Ok(crate::domain::truthy(&serde_json::from_str(raw.get())?)),
        }
    }

    pub fn string_equals(&self, key: &str, expected: &str) -> serde_json::Result<bool> {
        let Some(raw) = self.raw(key).filter(|raw| raw.get().starts_with('"')) else {
            return Ok(false);
        };
        Ok(serde_json::from_str::<JsonString>(raw.get())? == JsonString::from(expected))
    }

    pub fn get<T: de::DeserializeOwned>(&self, key: &str) -> serde_json::Result<Option<T>> {
        let key = JsonString::from(key);
        self.0
            .iter()
            .find(|(name, _)| name == &key)
            .map(|(_, raw)| serde_json::from_str(raw.get()))
            .transpose()
    }

    pub fn set(&mut self, key: &str, value: &impl Serialize) -> serde_json::Result<()> {
        let raw = serde_json::value::to_raw_value(value)?;
        self.insert(JsonString::from(key), raw);
        Ok(())
    }

    fn insert(&mut self, key: JsonString, raw: Box<RawValue>) {
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

    pub fn remove(&mut self, key: &str) {
        let key = JsonString::from(key);
        self.0.retain(|(name, _)| name != &key);
    }
}

impl Serialize for Document {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut json = String::from("{");
        for (index, (key, value)) in self.0.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            json.push_str(&key.escaped());
            json.push(':');
            json.push_str(value.get());
        }
        json.push('}');
        RawValue::from_string(json)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Document;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Document, M::Error> {
                let mut document = Document::default();
                while let Some((key, value)) = map.next_entry::<JsonString, Box<RawValue>>()? {
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
    fn utf16_strings_preserve_every_unit_and_escape_json_boundaries() {
        let original = JsonString((0..=u16::MAX).collect());
        let encoded = serde_json::to_string(&original).unwrap();
        assert_eq!(
            serde_json::from_str::<JsonString>(&encoded).unwrap(),
            original
        );
        assert!(original.utf8().is_err());
        for invalid in [r#""\ud800\x""#, r#""\udc0z""#, r#""unfinished"#] {
            assert!(serde_json::from_str::<JsonString>(invalid).is_err());
        }
    }

    #[test]
    fn document_edits_preserve_nested_values_keys_and_isolated_surrogates() {
        let raw = r#"{"id":"a","title":"\ud83d","\udc00":{"nested":["\ud800",null]},"future":1.2300,"status":"todo","status":"review"}"#;
        let mut document: Document = serde_json::from_str(raw).unwrap();
        assert_eq!(
            document.get::<String>("status").unwrap().as_deref(),
            Some("review")
        );
        assert_eq!(
            document.get::<JsonString>("title").unwrap(),
            Some(JsonString(vec![0xd83d]))
        );
        assert!(document.get::<String>("title").is_err());
        document.set("archived", &true).unwrap();
        document
            .set("title", &JsonString(vec![0xdc00, 0xd800, 0x61]))
            .unwrap();
        document.remove("status");
        let encoded = serde_json::to_string(&document).unwrap();
        assert!(encoded.contains(r#""\udc00":{"nested":["\ud800",null]}"#));
        assert!(encoded.contains(r#""future":1.2300"#));
        let reopened: Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            reopened.get::<JsonString>("title").unwrap(),
            Some(JsonString(vec![0xdc00, 0xd800, 0x61]))
        );
        assert_eq!(reopened.get::<bool>("archived").unwrap(), Some(true));
        assert!(reopened.get::<bool>("status").unwrap().is_none());
        for invalid in ["[]", "null", "{", r#"{"id":"x","future":INVALID}"#] {
            assert!(serde_json::from_str::<Document>(invalid).is_err());
        }
    }
}
