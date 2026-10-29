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
        static ENGLISH: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        static FRENCH: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        let english = ENGLISH.get_or_init(|| {
            serde_json::from_str(include_str!("../locales/en-US.json"))
                .expect("embedded translation catalog")
        });
        let french = FRENCH.get_or_init(|| {
            serde_json::from_str(include_str!("../locales/fr-FR.json"))
                .expect("embedded translation catalog")
        });
        let catalog = if self.0.get() == "en-US" {
            english
        } else {
            french
        };
        if let Some(translated) = catalog.get(source) {
            return translated.clone();
        }
        if self.0.get() != "en-US" || source.is_empty() {
            return source.to_owned();
        }
        // shortcut: some producers still send French text; remove this index when they emit keys.
        static LEGACY: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        let legacy = LEGACY.get_or_init(|| {
            french
                .iter()
                .map(|(key, message)| (message.clone(), key.clone()))
                .collect()
        });
        if let Some(key) = legacy.get(source) {
            return english[key].clone();
        }
        // System messages may contain a filename or a server detail; never translate user content.
        for (pattern, key) in legacy {
            let Some(translated) = english.get(key) else {
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
}

#[cfg(test)]
mod tests {
    use super::{two_arguments, worktree_parts};

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
