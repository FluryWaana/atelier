use atelier_app::infrastructure::jira::Jira;
use atelier_core::contracts::{JiraConfiguration, JiraMode};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[tokio::test]
async fn cloud_and_data_center_read_board_columns_under_the_configured_context_path() {
    for mode in [JiraMode::Cloud, JiraMode::DataCenter] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        let app=axum::Router::new().route("/jira/rest/agile/1.0/board/42/configuration",axum::routing::get(move||{let count=count.clone();async move{count.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"id":42,"name":"Synthetic board","columnConfig":{"columns":[{"name":"In progress","statuses":[{"id":"10"},{"id":"11"}]}]}}))}}));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let jira = Jira::synthetic(
            JiraConfiguration {
                mode,
                url: format!("http://{address}/jira"),
                email: "synthetic@example.test".into(),
                query: String::new(),
                revision: 7,
            },
            "synthetic-token".into(),
        )
        .unwrap();
        let result = jira.board_configuration(42).await.unwrap();
        assert_eq!(result.columns[0].status_ids, vec!["10", "11"]);
        assert_eq!(result.connection_revision, 7);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
