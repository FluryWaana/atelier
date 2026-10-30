use super::text;
use crate::document::Document;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub text: String,
    pub done: bool,
}

pub fn checklist(value: &RawValue) -> Result<Vec<Criterion>, &'static str> {
    let items = serde_json::from_str::<Vec<Box<RawValue>>>(value.get())
        .ok()
        .filter(|items| items.len() <= 20)
        .ok_or("message.maximum_20_acceptance_criteria")?;
    items
        .iter()
        .map(|item| {
            let invalid = "message.each_criterion_must_contain_text_and_a_valid_state";
            let item: Document = serde_json::from_str(item.get()).map_err(|_| invalid)?;
            let label = text(
                &item
                    .get::<String>("text")
                    .map_err(|_| invalid)?
                    .ok_or(invalid)?,
                500,
            )
            .map_err(|_| invalid)?;
            if label.is_empty() {
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

pub fn checklist_text(items: &[Criterion]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let lines = items
        .iter()
        .map(|item| format!("- [{}] {}", if item.done { "x" } else { " " }, item.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\nCritères de réussite (validation humaine) :\n{lines}")
}
