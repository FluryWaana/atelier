use super::text_units;
use crate::document::{Document, JsonString};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub text: JsonString,
    pub done: bool,
}

pub fn checklist(value: &RawValue) -> Result<Vec<Criterion>, &'static str> {
    let items = serde_json::from_str::<Vec<Box<RawValue>>>(value.get())
        .ok()
        .filter(|items| items.len() <= 20)
        .ok_or("Maximum 20 critères de réussite.")?;
    items
        .iter()
        .map(|item| {
            let invalid = "Chaque critère doit contenir un texte et un état valide.";
            let item: Document = serde_json::from_str(item.get()).map_err(|_| invalid)?;
            let label = text_units(
                &item
                    .get::<JsonString>("text")
                    .map_err(|_| invalid)?
                    .ok_or(invalid)?,
                500,
            )
            .map_err(|_| invalid)?;
            if label.0.is_empty() {
                return Err(invalid);
            }
            Ok(Criterion {
                text: label,
                done: item
                    .get::<bool>("done")
                    .map_err(|_| invalid)?
                    .ok_or(invalid)?,
            })
        })
        .collect()
}

pub fn checklist_text(items: &[Criterion]) -> JsonString {
    if items.is_empty() {
        return JsonString::default();
    }
    let mut result = JsonString::from("\n\nCritères de réussite (validation humaine) :\n");
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            result.0.push(u16::from(b'\n'));
        }
        result
            .0
            .extend(if item.done { "- [x] " } else { "- [ ] " }.encode_utf16());
        result.0.extend_from_slice(&item.text.0);
    }
    result
}
