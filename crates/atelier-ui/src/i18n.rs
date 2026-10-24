use leptos::prelude::*;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy)]
pub struct I18n(RwSignal<&'static str>);

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
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
            .ok_or("La langue change pour cette fenêtre, mais ne peut pas être enregistrée dans ce navigateur.")?
            .set_item("atelier-language", self.0.get_untracked())
            .map_err(|_| "La langue change pour cette fenêtre, mais ne peut pas être enregistrée dans ce navigateur.")
    }

    pub fn text(self, source: &str) -> String {
        if self.0.get() != "en-US" || source.is_empty() {
            return source.to_owned();
        }
        static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        let catalog = CATALOG.get_or_init(|| {
            serde_json::from_str(include_str!("../locales/en-US.json"))
                .expect("embedded translation catalog")
        });
        if let Some(translated) = catalog.get(source) {
            return translated.clone();
        }
        // System messages may contain a filename or a server detail; never translate user content.
        for (pattern, translated) in catalog.iter() {
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
