#![cfg(windows)]
use atelier_app::infrastructure::agent_cli::Catalog;
use axum::{
    Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};

async fn published(State(fault): State<&'static str>, request: Request) -> Response {
    assert_eq!(request.method(), axum::http::Method::GET);
    assert_eq!(request.uri().path(), "/@openai%2Fcodex/latest");
    assert!(request.uri().query().is_none());
    assert!(!request.headers().contains_key("authorization"));
    assert!(!request.headers().contains_key("cookie"));
    match fault {
        "redirect" => (
            axum::http::StatusCode::FOUND,
            [("location", "/must-not-follow")],
        )
            .into_response(),
        "large" => "x".repeat(65_537).into_response(),
        "wrong" => r#"{"name":"@anthropic-ai/claude-code","version":"2.0.0"}"#.into_response(),
        "unstable" => r#"{"name":"@openai/codex","version":"2.0.0-beta.1"}"#.into_response(),
        "invalid" => "invalid json".into_response(),
        _ => r#"{"name":"@openai/codex","version":"2.0.0"}"#.into_response(),
    }
}

#[tokio::test]
async fn catalog_reads_only_the_exact_package_and_rejects_redirects_or_invalid_versions() {
    for fault in ["valid", "redirect", "large", "wrong", "unstable", "invalid"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(published).with_state(fault),
            )
            .await
            .unwrap();
        });
        let catalog = Catalog::synthetic(&format!("http://{address}/")).unwrap();
        let result = catalog.latest("codex").await;
        if fault == "valid" {
            assert_eq!(result.unwrap(), "2.0.0");
        } else {
            assert!(result.is_err(), "{fault}");
        }
        assert!(catalog.latest("command").await.is_err());
        server.abort();
    }
}
