#[path = "support/tls.rs"]
mod tls;
use atelier_app::{
    Result,
    infrastructure::{
        database::Database, forge::Forge, jira::Jira, jira_import, store::Documents, tempo::Tempo,
        tempo_export, time_tracking::Clock,
    },
};
use atelier_core::{
    application::time_tracking,
    contracts::{
        Card, ForgeConfiguration, ForgeKind, ForgeWriteState, JiraConfiguration, JiraImport,
        JiraMode, TempoConfiguration, TempoExportIntent, TempoExportRequest, TempoPayload,
    },
    document::Document,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct Remote {
    stored: Arc<Mutex<Option<TempoPayload>>>,
    writes: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    unexpected: Arc<AtomicUsize>,
}
fn row(payload: &TempoPayload) -> Value {
    json!({"tempoWorklogId":7,"issue":{"id":payload.issue_id},"author":{"accountId":payload.author_account_id},"timeSpentSeconds":payload.time_spent_seconds,"startDate":payload.start_date,"startTime":payload.start_time,"description":payload.description})
}
fn authorized(request: &Request, expected: &str) {
    assert!(
        request
            .headers()
            .get("authorization")
            .is_some_and(|header| header == expected)
    );
    assert!(!request.headers().contains_key("cookie"));
}
async fn remote(State(state): State<Remote>, request: Request) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let path = request.uri().path().to_owned();
    match path.as_str() {
        "/jira/rest/api/3/issue/123" => {
            assert_eq!(request.method(), Method::GET);
            authorized(
                &request,
                &format!(
                    "Basic {}",
                    STANDARD.encode("synthetic@example.invalid:synthetic-jira-token")
                ),
            );
            assert!(
                request
                    .uri()
                    .query()
                    .is_some_and(|query| query.contains("fields=summary%2Cstatus%2Cdescription"))
            );
            Json(json!({"id":"123","key":"SYN-1","fields":{"summary":"TLS synthetic issue","status":{"id":"1","name":"Open"},"description":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"TLS description é🦀"}]}]}}})).into_response()
        }
        "/jira/rest/api/3/myself" => {
            assert_eq!(request.method(), Method::GET);
            authorized(
                &request,
                &format!(
                    "Basic {}",
                    STANDARD.encode("synthetic@example.invalid:synthetic-jira-token")
                ),
            );
            Json(json!({"accountId":"synthetic-account"})).into_response()
        }
        "/api/v3/repos/owner/repo/pulls/7" => {
            assert_eq!(request.method(), Method::GET);
            authorized(&request, "Bearer synthetic-forge-token");
            assert!(
                request
                    .headers()
                    .get("x-github-api-version")
                    .is_some_and(|value| value == "2022-11-28")
            );
            Json(json!({"number":7,"title":"TLS synthetic PR","state":"open","merged":false,"draft":true,"head":{"ref":"task","sha":"a".repeat(40),"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":"b".repeat(40),"repo":{"full_name":"owner/repo"}},"html_url":"https://127.0.0.1:1/never-follow"})).into_response()
        }
        "/tempo/4/worklogs" if request.method() == Method::POST => {
            authorized(&request, "Bearer synthetic-tempo-token");
            assert_eq!(
                state.writes.fetch_add(1, Ordering::SeqCst),
                0,
                "No second synthetic write is permitted"
            );
            let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024)
                .await
                .unwrap();
            let payload: TempoPayload = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(payload.issue_id, 123);
            assert_eq!(payload.author_account_id, "synthetic-account");
            *state.stored.lock().unwrap() = Some(payload);
            // The effect exists before the TLS response body fails; the client cannot know its ID.
            let body = Body::from_stream(futures_util::stream::once(async {
                Err::<hyper::body::Bytes, std::io::Error>(std::io::Error::other(
                    "Synthetic lost TLS response after one write",
                ))
            }));
            axum::http::Response::builder()
                .status(StatusCode::CREATED)
                .header("content-type", "application/json")
                .body(body)
                .unwrap()
        }
        "/tempo/4/worklogs/user/synthetic-account" => {
            assert_eq!(request.method(), Method::GET);
            authorized(&request, "Bearer synthetic-tempo-token");
            state.reads.fetch_add(1, Ordering::SeqCst);
            let url = reqwest::Url::parse(&format!("https://synthetic.invalid{}", request.uri()))
                .unwrap();
            let query = url
                .query_pairs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(query.get("limit").map(|value| value.as_ref()), Some("100"));
            let payload = state.stored.lock().unwrap().clone().unwrap();
            assert_eq!(
                query.get("from").map(|value| value.as_ref()),
                Some(payload.start_date.as_str())
            );
            assert_eq!(
                query.get("to").map(|value| value.as_ref()),
                Some(payload.start_date.as_str())
            );
            let results = match query.get("offset").map(|value| value.as_ref()) {
                Some("0") => vec![row(&payload)],
                Some("1") => Vec::new(),
                _ => panic!("Unexpected synthetic pagination"),
            };
            Json(json!({"results":results,"metadata":{"next":null}})).into_response()
        }
        "/tempo/4/worklogs/7" => {
            assert_eq!(request.method(), Method::GET);
            authorized(&request, "Bearer synthetic-tempo-token");
            state.reads.fetch_add(1, Ordering::SeqCst);
            Json(row(state.stored.lock().unwrap().as_ref().unwrap())).into_response()
        }
        _ => {
            state.unexpected.fetch_add(1, Ordering::SeqCst);
            (StatusCode::NOT_FOUND, "Unexpected synthetic scope").into_response()
        }
    }
}
fn document(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn connection(base: &str) -> JiraConfiguration {
    JiraConfiguration {
        mode: JiraMode::Cloud,
        url: format!("{base}/jira"),
        email: "synthetic@example.invalid".into(),
        query: "project = SYN".into(),
        revision: 1,
    }
}
fn selection() -> JiraImport {
    JiraImport {
        revision: 1,
        issue_id: "123".into(),
        project_id: "synthetic-project".into(),
        attach_to: None,
    }
}

#[tokio::test]
#[ignore = "requires CI final campaign and ephemeral synthetic TLS authority"]
async fn jira_time_tempo_round_trip_uses_real_tls_and_never_replays_a_lost_write() -> Result<()> {
    tls::require_final();
    let certificates = tls::Certificates::new().await?;
    let state = Remote::default();
    let server = tls::Server::start(
        certificates.valid.clone(),
        Router::new().fallback(remote).with_state(state.clone()),
    )
    .await?;
    let configuration = connection(&server.base);
    let tempo_configuration = TempoConfiguration {
        base: format!("{}/tempo/4", server.base),
        revision: 1,
    };
    let jira = Jira::synthetic_tls(
        configuration.clone(),
        "synthetic-jira-token".into(),
        &certificates.authority,
    )?;
    let tempo = Tempo::synthetic_tls(
        &tempo_configuration.base,
        "synthetic-tempo-token".into(),
        &certificates.authority,
    )?;
    let forge = Forge::synthetic_tls(
        ForgeConfiguration {
            kind: ForgeKind::Github,
            base: server.base.clone(),
        },
        "synthetic-forge-token".into(),
        1,
        &certificates.authority,
    )?;
    let pr = forge.pull_request("owner/repo", 7).await?;
    assert_eq!(pr.repository, "owner/repo");
    assert_eq!(pr.base_url, server.base);
    assert_eq!(pr.url, format!("{}/owner/repo/pull/7", server.base));
    let home = tempfile::tempdir()?;
    let path = home.path().join("synthetic.sqlite");
    let repository = home.path().join("synthetic-repository");
    std::fs::create_dir(&repository)?;
    let database = Database::open(&path)?;
    let stored = configuration.clone();
    let stored_tempo = tempo_configuration.clone();
    database
        .call(move |store| {
            store.save_settings(&document(
                json!({"jiraConnection":stored,"tempoConnection":stored_tempo}),
            ))?;
            store.put(
                Documents::Projects,
                &document(json!({"id":"synthetic-project","name":"Synthetic","repo":repository})),
            )?;
            jira_import::reserve(store, selection(), "first".into())
        })
        .await?;
    let detail = jira.detail("123").await?;
    assert_eq!(detail.description, "TLS description é🦀");
    let mut card = database
        .call(move |store| Ok(jira_import::finish(store, "first", detail)?.card))
        .await?;
    card.fields.set(
        "description",
        &"Human notes retained across TLS import retry",
    )?;
    card.title = atelier_core::document::JsonString::from("Human title stays local");
    card.fields.set(
        "checklist",
        &json!([{"text":"Human accepted criterion","done":true}]),
    )?;
    let edited = card.clone();
    database
        .call(move |store| {
            store.put(Documents::Cards, &edited)?;
            jira_import::reserve(store, selection(), "retry".into())
        })
        .await?;
    let detail = jira.detail("123").await?;
    let card = database
        .call(move |store| {
            let imported = jira_import::finish(store, "retry", detail)?;
            assert!(!imported.created);
            assert!(imported.card.fields.string_equals(
                "description",
                "Human notes retained across TLS import retry"
            )?);
            assert_eq!(store.documents::<Card>(Documents::Cards)?.len(), 1);
            Ok(imported.card)
        })
        .await?;
    let entry_id = uuid::Uuid::new_v4().to_string();
    let identifier = entry_id.clone();
    let card_id = card.id.clone();
    let entry=database.call(move|store|time_tracking::execute(store,&Clock,"create","",&document(json!({"cardId":card_id,"seconds":120,"date":"2026-10-26","startTime":"09:30:00","description":"Human TLS time é🦀"})),||identifier,|_|false)).await?;
    let account = jira.account_id().await?;
    let payload = TempoPayload {
        issue_id: 123,
        author_account_id: account,
        time_spent_seconds: 120,
        start_date: "2026-10-26".into(),
        start_time: "09:30:00".into(),
        description: format!("Human TLS time é🦀\n[atelier:{entry_id}]"),
    };
    let input = TempoExportRequest {
        expected: entry,
        expected_card: card.clone(),
        jira_revision: 1,
        tempo_revision: 1,
        confirmed: false,
    };
    let intended = payload.clone();
    let mut input = database
        .call(move |store| {
            let mut input = input;
            input.expected =
                tempo_export::save_preview(store, &input, tempo_configuration, intended)?;
            Ok(input)
        })
        .await?;
    let operation = uuid::Uuid::new_v4().to_string();
    let approved = operation.clone();
    input.confirmed = true;
    database
        .call(move |store| {
            tempo_export::reserve(store, &input, &approved)?;
            assert!(tempo_export::reserve(store, &input, &approved).is_err());
            Ok(())
        })
        .await?;
    assert!(tempo.create(&payload).await.is_err());
    assert_eq!(state.writes.load(Ordering::SeqCst), 1);
    assert_eq!(state.stored.lock().unwrap().as_ref(), Some(&payload));
    // Reopen actual SQLite while the synthetic remote effect remains alive, then reconcile read-only.
    database.close().await?;
    let database = Database::open(&path)?;
    let selected = entry_id.clone();
    database
        .call(move |store| {
            time_tracking::initialize(store)?;
            let pending = store
                .document::<Document>(Documents::Times, &selected)?
                .unwrap();
            assert!(pending.string_equals("exportState", "uncertain")?);
            let intent = pending.get::<TempoExportIntent>("tempoExport")?.unwrap();
            assert_eq!(intent.state, ForgeWriteState::Uncertain);
            assert!(intent.worklog_id.is_none());
            assert!(tempo_export::blocks(store, "/api/backups")?);
            Ok(())
        })
        .await?;
    let observed = tempo
        .find(&entry_id, &payload)
        .await?
        .ok_or("Synthetic lost write was not observed")?;
    let selected = entry_id.clone();
    let approved = operation.clone();
    database
        .call(move |store| tempo_export::remember(store, &selected, &approved, observed))
        .await?;
    let observed = tempo.read(observed, &payload).await?;
    let selected = entry_id.clone();
    let approved = operation.clone();
    let expected_card = card.clone();
    database
        .call(move |store| {
            let exported = tempo_export::record(store, &selected, &approved, observed, 1, 1)?;
            assert!(exported.string_equals("exportState", "exported")?);
            assert_eq!(exported.get::<u64>("seconds")?, Some(120));
            assert_eq!(
                exported
                    .get::<TempoExportIntent>("tempoExport")?
                    .unwrap()
                    .state,
                ForgeWriteState::Confirmed
            );
            assert_eq!(
                serde_json::to_value(
                    store
                        .document::<Card>(Documents::Cards, &expected_card.id)?
                        .unwrap()
                )?,
                serde_json::to_value(expected_card)?
            );
            Ok(())
        })
        .await?;
    assert_eq!(state.writes.load(Ordering::SeqCst), 1);
    assert_eq!(state.reads.load(Ordering::SeqCst), 3);
    assert_eq!(state.unexpected.load(Ordering::SeqCst), 0);
    database.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires CI final campaign and ephemeral synthetic TLS authority"]
async fn synthetic_tls_rejects_foreign_authorities_wrong_names_and_nonloopback_scopes() -> Result<()>
{
    tls::require_final();
    let certificates = tls::Certificates::new().await?;
    let state = Remote::default();
    let server = tls::Server::start(
        certificates.valid.clone(),
        Router::new().fallback(remote).with_state(state.clone()),
    )
    .await?;
    let unknown = Jira::synthetic_tls(
        connection(&server.base),
        "synthetic-jira-token".into(),
        &certificates.foreign,
    )?;
    assert!(unknown.detail("123").await.is_err());
    let wrong = tls::Server::start(
        certificates.wrong_host.clone(),
        Router::new().fallback(remote).with_state(state.clone()),
    )
    .await?;
    let wrong_name = Jira::synthetic_tls(
        connection(&wrong.base),
        "synthetic-jira-token".into(),
        &certificates.authority,
    )?;
    assert!(wrong_name.detail("123").await.is_err());
    for address in [
        "https://example.invalid:444/jira",
        "http://127.0.0.1:444/jira",
        "https://user:synthetic-token@127.0.0.1:444/jira",
        "https://127.0.0.1:444/jira?scope=foreign",
    ] {
        assert!(
            Jira::synthetic_tls(
                connection(address),
                "synthetic-jira-token".into(),
                &certificates.authority
            )
            .is_err()
        );
    }
    assert!(
        Tempo::synthetic_tls(
            &format!("{}/tempo/4", server.base),
            "unmarked-token".into(),
            &certificates.authority
        )
        .is_err()
    );
    assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    Ok(())
}
