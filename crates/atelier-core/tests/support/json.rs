use atelier_core::document::Document;
use serde_json::{Value, value::RawValue};

pub fn equivalent(actual: &RawValue, expected: &RawValue) -> bool {
    match (actual.get().as_bytes()[0], expected.get().as_bytes()[0]) {
        (b'{', b'{') => {
            let actual: Document = serde_json::from_str(actual.get()).unwrap();
            let expected: Document = serde_json::from_str(expected.get()).unwrap();
            actual.keys().count() == expected.keys().count()
                && actual.keys().all(|key| {
                    expected
                        .raw(key)
                        .is_some_and(|expected| equivalent(actual.raw(key).unwrap(), expected))
                })
        }
        (b'[', b'[') => {
            let actual: Vec<Box<RawValue>> = serde_json::from_str(actual.get()).unwrap();
            let expected: Vec<Box<RawValue>> = serde_json::from_str(expected.get()).unwrap();
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, e)| equivalent(a, e))
        }
        (b'"', b'"') => {
            serde_json::from_str::<String>(actual.get()).unwrap()
                == serde_json::from_str::<String>(expected.get()).unwrap()
        }
        _ => {
            serde_json::from_str::<Value>(actual.get()).unwrap()
                == serde_json::from_str::<Value>(expected.get()).unwrap()
        }
    }
}
