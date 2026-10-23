pub mod agent_options;
pub mod checklist;
pub mod projects;
pub mod tasks;

pub(crate) fn truthy(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

pub fn text(value: &str, limit: usize) -> Result<String, &'static str> {
    if value.encode_utf16().count() > limit {
        return Err("Texte invalide ou trop long.");
    }
    // ECMAScript trim also removes BOM and does not remove NEL.
    Ok(value.trim_matches(|c: char| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_javascript_lengths_and_trim() {
        assert_eq!(text("\u{feff} é 🔧 \u{00a0}", 20), Ok("é 🔧".into()));
        assert!(text(&"🔧".repeat(151), 300).is_err());
        assert!(text(&"🔧".repeat(150), 300).is_ok());
        assert_eq!(text("\u{85}x\u{85}", 10), Ok("\u{85}x\u{85}".into()));
    }
}
