use super::text;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub text: String,
    pub done: bool,
}

pub fn checklist(value: &Value) -> Result<Vec<Criterion>, &'static str> {
    let items = value
        .as_array()
        .filter(|items| items.len() <= 20)
        .ok_or("Maximum 20 critères de réussite.")?;
    items
        .iter()
        .map(|item| {
            let invalid = "Chaque critère doit contenir un texte et un état valide.";
            let label = text(item["text"].as_str().ok_or(invalid)?, 500).map_err(|_| invalid)?;
            if label.is_empty() {
                return Err(invalid);
            }
            Ok(Criterion {
                text: label,
                done: item["done"].as_bool().ok_or(invalid)?,
            })
        })
        .collect()
}

pub fn checklist_text(items: &[Criterion]) -> String {
    if items.is_empty() {
        return String::new();
    }
    format!(
        "\n\nCritères de réussite (validation humaine) :\n{}",
        items
            .iter()
            .map(|item| format!("- [{}] {}", if item.done { 'x' } else { ' ' }, item.text))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
