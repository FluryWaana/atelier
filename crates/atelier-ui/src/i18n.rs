use leptos::prelude::*;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy)]
pub struct I18n(RwSignal<&'static str>);

fn worktree_parts(message: &str) -> Option<(&str, &str)> {
    let (role, cause) = message.strip_prefix("Worktree ")?.split_once(" : ")?;
    matches!(role, "front" | "back").then_some((role, cause))
}

fn two_arguments(template: &str, first: &str, second: &str) -> String {
    if let Some((prefix, rest)) = template.split_once("{0}")
        && let Some((middle, suffix)) = rest.split_once("{1}")
    {
        return format!("{prefix}{first}{middle}{second}{suffix}");
    }
    if let Some((prefix, rest)) = template.split_once("{1}")
        && let Some((middle, suffix)) = rest.split_once("{0}")
    {
        return format!("{prefix}{second}{middle}{first}{suffix}");
    }
    template.to_owned()
}

fn locale(value: &str) -> &'static str {
    if value == "en-US" { "en-US" } else { "fr-FR" }
}

fn catalog(language: &str) -> &'static BTreeMap<String, String> {
    static ENGLISH: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    static FRENCH: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let (catalog, source) = if language == "en-US" {
        (&ENGLISH, include_str!("../locales/en-US.json"))
    } else {
        (&FRENCH, include_str!("../locales/fr-FR.json"))
    };
    catalog.get_or_init(|| serde_json::from_str(source).expect("embedded translation catalog"))
}

fn translate(language: &str, source: &str) -> String {
    let translations = catalog(language);
    if let Some(translated) = translations.get(source) {
        return translated.clone();
    }
    if language != "en-US" || source.is_empty() {
        return source.to_owned();
    }
    // shortcut: some producers still send French text; remove this index when they emit keys.
    static LEGACY: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();
    let legacy = LEGACY.get_or_init(|| {
        catalog("fr-FR")
            .iter()
            .map(|(key, message)| (message.as_str(), key.as_str()))
            .collect()
    });
    if let Some(key) = legacy.get(source)
        && let Some(translated) = translations.get(*key)
    {
        return translated.clone();
    }
    // System messages may contain a filename or a server detail; never translate user content.
    for (pattern, key) in legacy {
        let Some(translated) = translations.get(*key) else {
            continue;
        };
        if let Some((prefix, suffix)) = pattern.split_once("{0}")
            && !suffix.contains('{')
            && let Some(value) = source
                .strip_prefix(prefix)
                .and_then(|value| value.strip_suffix(suffix))
        {
            return translated.replace("{0}", value);
        }
    }
    source.to_owned()
}

impl Default for I18n {
    fn default() -> Self {
        let saved = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item("atelier-language").ok().flatten())
            .unwrap_or_default();
        let language = Self(RwSignal::new(locale(&saved)));
        language.document_language();
        language
    }
}

impl I18n {
    // shortcut: the API still combines role and cause; replace parsing when it returns structured errors.
    pub fn worktree_error(self, message: &str) -> String {
        if let Some((role, cause)) = worktree_parts(message) {
            return two_arguments(
                &self.text("message.worktree_role_error"),
                role,
                &self.worktree_cause(cause),
            );
        }
        self.worktree_cause(message)
    }

    fn worktree_cause(self, message: &str) -> String {
        if let Some((cause, restoration)) =
            message.split_once(" Restauration des liens impossible : ")
        {
            return two_arguments(
                &self.text("message.link_restoration_failed"),
                &self.text(cause),
                &self.text(restoration),
            );
        }
        self.text(message)
    }

    pub fn code(self) -> &'static str {
        self.0.get()
    }

    pub fn date(self, milliseconds: f64) -> String {
        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(milliseconds));
        if !date.get_time().is_finite() {
            return self.text("message.unavailable");
        }
        date.to_locale_date_string(self.code(), &wasm_bindgen::JsValue::UNDEFINED)
            .into()
    }

    fn document_language(self) {
        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
        {
            let _ = element.set_attribute("lang", self.0.get_untracked());
        }
    }

    pub fn select(self, value: &str) -> Result<(), &'static str> {
        self.0.set(locale(value));
        self.document_language();
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or("message.the_language_changed_for_this_window_but_cannot_be_saved_in_this")?
            .set_item("atelier-language", self.0.get_untracked())
            .map_err(|_| "message.the_language_changed_for_this_window_but_cannot_be_saved_in_this")
    }

    pub fn text(self, source: &str) -> String {
        translate(self.0.get(), source)
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog, translate, two_arguments, worktree_parts};

    #[test]
    fn translations_preserve_catalogs_legacy_messages_and_user_content() {
        for language in ["fr-FR", "en-US"] {
            for (key, message) in catalog(language) {
                assert_eq!(translate(language, key), *message);
            }
            let unknown = ["message", "synthetic_unknown"].join(".");
            for source in ["", "Synthetic user task", &unknown] {
                assert_eq!(translate(language, source), source);
            }
        }
        let french = catalog("fr-FR");
        let english = catalog("en-US");
        let mut patterns = 0;
        for (key, pattern) in french {
            if let Some((_, suffix)) = pattern.split_once("{0}")
                && !suffix.contains('{')
            {
                let source = pattern.replace("{0}", "synthetic-file.txt");
                assert_eq!(
                    translate("en-US", &source),
                    english[key].replace("{0}", "synthetic-file.txt")
                );
                patterns += 1;
            }
        }
        assert!(patterns > 0);
        assert_eq!(
            translate("en-US", &french["message.save"]),
            english["message.save"]
        );
    }

    #[test]
    fn worktree_error_keeps_the_role_and_full_cause() {
        assert_eq!(
            worktree_parts("Worktree front : message.invalid_file_path"),
            Some(("front", "message.invalid_file_path"))
        );
        assert_eq!(
            worktree_parts("Worktree back : detail : extra"),
            Some(("back", "detail : extra"))
        );
        assert_eq!(
            worktree_parts("Worktree other : message.invalid_file_path"),
            None
        );
        assert_eq!(
            two_arguments("{0} / {1}", "path {1}", "cause {0}"),
            "path {1} / cause {0}"
        );
        assert_eq!(
            two_arguments("{1} / {0}", "front", "cause"),
            "cause / front"
        );
    }
}
