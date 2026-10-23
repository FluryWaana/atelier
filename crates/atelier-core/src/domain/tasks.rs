use super::truthy;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub const STATUSES: &[&str] = &["todo", "doing", "review", "done", "standby"];

pub fn review_requirements(card: &Value) -> Value {
    let content = |key| {
        card.get(key)
            .filter(|value| truthy(value))
            .cloned()
            .unwrap_or(json!(""))
    };
    json!({
        "title": content("title"),
        "description": content("description"),
        "instructions": content("instructions"),
        "criteria": card["checklist"].as_array().map(|items| items.iter().map(|item| item["text"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
    })
}

pub fn blockers(card: &Value, cards: &[Value]) -> Vec<Value> {
    card["blockedBy"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|id| {
            cards
                .iter()
                .find(|card| card.get("id") == Some(id))
                .cloned()
                .unwrap_or_else(
                    || json!({"id": id, "title": "Tâche introuvable", "status": "missing"}),
                )
        })
        .filter(|card| card["status"] != "done")
        .collect()
}

pub fn dependencies(id: &str, value: &Value, cards: &[Value]) -> Result<Vec<String>, &'static str> {
    let invalid =
        "Dépendances invalides (20 tâches maximum, sans doublon ni référence à soi-même).";
    let by_id: HashMap<_, _> = cards
        .iter()
        .filter_map(|card| Some((card["id"].as_str()?, card)))
        .collect();
    let items = value
        .as_array()
        .filter(|items| items.len() <= 20)
        .ok_or(invalid)?;
    let mut unique = HashSet::new();
    let mut selected = Vec::with_capacity(items.len());
    for item in items {
        let key = item.as_str().ok_or(invalid)?;
        if key == id || !by_id.contains_key(key) || !unique.insert(key) {
            return Err(invalid);
        }
        selected.push(key);
    }
    // Iteration avoids a stack overflow for a long chain of existing tasks.
    let mut pending = selected.clone();
    let mut seen = HashSet::new();
    while let Some(key) = pending.pop() {
        if key == id {
            return Err("Cette dépendance créerait un cycle.");
        }
        if !seen.insert(key) {
            continue;
        }
        if let Some(next) = by_id.get(key).and_then(|card| card["blockedBy"].as_array()) {
            pending.extend(next.iter().filter_map(Value::as_str));
        }
    }
    Ok(selected.into_iter().map(str::to_owned).collect())
}
