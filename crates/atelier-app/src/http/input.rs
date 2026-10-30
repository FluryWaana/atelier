use super::{failure, object};
use axum::{extract::Request, http::StatusCode, response::Response};
use serde::de::DeserializeOwned;

pub(super) async fn parse<T: DeserializeOwned>(
    request: Request,
) -> std::result::Result<T, Box<Response>> {
    let body = object(request.into_body())
        .await
        .map_err(|(status, message)| Box::new(failure(status, message)))?;
    serde_json::from_str(&serde_json::to_string(&body).map_err(|_| {
        Box::new(failure(
            StatusCode::BAD_REQUEST,
            "message.invalid_request_parameters",
        ))
    })?)
    .map_err(|_| {
        Box::new(failure(
            StatusCode::BAD_REQUEST,
            "message.invalid_request_parameters",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        name: String,
    }

    async fn read(body: &str) -> std::result::Result<Input, Box<Response>> {
        parse(Request::new(Body::from(body.to_owned()))).await
    }

    #[tokio::test]
    async fn typed_requests_preserve_unicode_and_reject_invalid_or_excessive_input() {
        assert_eq!(
            read(r#"{"name":"Tâche 🔧"}"#).await.unwrap().name,
            "Tâche 🔧"
        );
        for body in [
            "[]",
            "null",
            "{",
            r#"{"name":false}"#,
            r#"{"name":"\ud800"}"#,
            r#"{"name":"valid","extra":true}"#,
        ] {
            assert_eq!(
                read(body).await.unwrap_err().status(),
                StatusCode::BAD_REQUEST
            );
        }
        let body = format!(r#"{{"name":"{}"}}"#, "a".repeat(400_000));
        assert_eq!(
            read(&body).await.unwrap_err().status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
