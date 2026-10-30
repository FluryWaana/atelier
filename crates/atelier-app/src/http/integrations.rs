use super::{Shared, responses::response};
use atelier_core::{
    application::integrations::{self, Kind},
    document::Document,
};
use axum::{
    extract::{Request, State},
    response::Response,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    kind: Kind,
    provider: String,
    expected: String,
}

pub(super) async fn choose(State(state): State<Shared>, request: Request) -> Response {
    let input: Choice = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let result = state
        .database
        .call(move |store| {
            let settings = store.settings::<Document>()?;
            let next =
                integrations::choose(&settings, input.kind, &input.provider, &input.expected)?;
            store.save_settings(&next)?;
            Ok(serde_json::json!({"provider":input.provider}))
        })
        .await;
    if result.is_ok() {
        let _ = state.changed.send(());
    }
    response(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choice_accepts_only_a_typed_category_and_exact_fields() {
        assert!(
            serde_json::from_str::<Choice>(
                r#"{"kind":"tickets","provider":"local","expected":"jira"}"#
            )
            .is_ok()
        );
        for invalid in [
            r#"{"kind":"jira","provider":"local","expected":"jira"}"#,
            r#"{"kind":"tickets","provider":true,"expected":"local"}"#,
            r#"{"kind":"tickets","provider":"local","expected":"local","token":"synthetic"}"#,
        ] {
            assert!(serde_json::from_str::<Choice>(invalid).is_err());
        }
    }
}
