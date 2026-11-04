use super::{Shared, failure, object};
use crate::infrastructure::settings;
use axum::{
    Json,
    extract::{Path, Request, State},
    response::{IntoResponse, Response},
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageInput {
    language: atelier_core::contracts::InterfaceLanguage,
    expected: Option<atelier_core::contracts::InterfaceLanguage>,
}

pub(super) async fn language(State(state): State<Shared>, request: Request) -> Response {
    let input: LanguageInput = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let language = input.language;
    let updates = state.interface_language.clone();
    match state
        .database
        .call(move |store| {
            settings::update_language(store, language, input.expected)?;
            updates.send_replace(language);
            Ok(())
        })
        .await
    {
        Ok(()) => Json(language).into_response(),
        Err(error) => super::responses::error(error, "message.the_settings_could_not_be_updated"),
    }
}

async fn execute(state: Shared, service: Option<String>, request: Request) -> Response {
    let vault = state.vault.clone();
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| match service {
            Some(service) => settings::remove_token(store, &service, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            }),
            None => settings::update(store, &body, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            }),
        })
        .await
    {
        Ok(config) => Json(config).into_response(),
        Err(error) => super::responses::error(error, "message.the_settings_could_not_be_updated"),
    }
}

pub(super) async fn update(State(state): State<Shared>, request: Request) -> Response {
    execute(state, None, request).await
}

pub(super) async fn remove_token(
    State(state): State<Shared>,
    Path(service): Path<String>,
    request: Request,
) -> Response {
    execute(state, Some(service), request).await
}

#[cfg(test)]
mod tests {
    use super::LanguageInput;

    #[test]
    fn language_requests_reject_unsupported_values_and_extra_fields() {
        for language in ["fr-FR", "en-US"] {
            let input: LanguageInput =
                serde_json::from_value(serde_json::json!({"language":language,"expected":null}))
                    .unwrap();
            assert_eq!(input.language.code(), language);
        }
        for value in [
            serde_json::json!({"language":"de-DE"}),
            serde_json::json!({"language":true}),
            serde_json::json!({"language":"en-US","expected":"unsupported"}),
            serde_json::json!({"language":"en-US","token":"synthetic"}),
        ] {
            assert!(serde_json::from_value::<LanguageInput>(value).is_err());
        }
    }
}
