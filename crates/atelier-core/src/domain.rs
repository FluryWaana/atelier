pub mod agent_options;
pub mod checklist;
pub mod projects;
pub mod tasks;
pub mod time;

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

fn whitespace(value: u32) -> bool {
    matches!(value, 0x0009..=0x000d | 0x0020 | 0x00a0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
}

pub fn text_units(
    value: &crate::document::JsonString,
    limit: usize,
) -> Result<crate::document::JsonString, &'static str> {
    if value.0.len() > limit {
        return Err("Texte invalide ou trop long.");
    }
    let start = value
        .0
        .iter()
        .position(|unit| !whitespace(u32::from(*unit)))
        .unwrap_or(value.0.len());
    let end = value
        .0
        .iter()
        .rposition(|unit| !whitespace(u32::from(*unit)))
        .map_or(start, |index| index + 1);
    Ok(crate::document::JsonString(value.0[start..end].to_vec()))
}

pub fn text(value: &str, limit: usize) -> Result<String, &'static str> {
    if value.encode_utf16().count() > limit {
        return Err("Texte invalide ou trop long.");
    }
    // ECMAScript trim also removes BOM and does not remove NEL.
    Ok(value
        .trim_matches(|c: char| whitespace(u32::from(c)))
        .to_owned())
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
        use crate::document::JsonString;
        assert_eq!(
            text_units(&JsonString(vec![0xfeff, 0xd800, 0xa0]), 3).unwrap(),
            JsonString(vec![0xd800])
        );
        assert_eq!(
            text_units(&JsonString(vec![0xa0, 0xfeff]), 2).unwrap(),
            JsonString(vec![])
        );
        assert_eq!(
            text_units(&JsonString(vec![0x85]), 1).unwrap(),
            JsonString(vec![0x85])
        );
        assert!(text_units(&JsonString(vec![0xd800; 301]), 300).is_err());
    }
}
