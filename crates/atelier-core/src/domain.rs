pub mod agent_catalog;
pub mod agent_context;
pub mod agent_options;
pub mod checklist;
pub mod files;
pub mod projects;
pub mod tasks;
pub mod time;

fn whitespace(value: u32) -> bool {
    matches!(value, 0x0009..=0x000d | 0x0020 | 0x00a0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
}

pub fn text(value: &str, limit: usize) -> Result<String, &'static str> {
    if value.encode_utf16().count() > limit {
        return Err("message.invalid_or_overly_long_text");
    }
    // Treat pasted BOMs and surrounding spaces as whitespace.
    Ok(value
        .trim_matches(|c: char| whitespace(u32::from(c)))
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_bounds_match_browser_maxlength_without_splitting_unicode() {
        assert_eq!(text("\u{feff} é 🔧 \u{00a0}", 20), Ok("é 🔧".into()));
        assert!(text(&"🔧".repeat(151), 300).is_err());
        assert!(text(&"🔧".repeat(150), 300).is_ok());
    }
}
