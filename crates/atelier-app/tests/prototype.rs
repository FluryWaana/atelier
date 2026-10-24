use atelier_app::{
    http,
    infrastructure::{database::Database, prototype::PrototypeDirectory, session},
};
use atelier_core::contracts::Card;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn start(
    data: &Path,
    assets: &Path,
) -> (
    String,
    String,
    tokio::task::JoinHandle<atelier_app::Result<()>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(http::serve(listener, data.to_owned(), assets.to_owned()));
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(token) = session::read(data)
            && client
                .get(format!("{address}/api/state"))
                .bearer_auth(&token)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        {
            return (address, token, task);
        }
        assert!(!task.is_finished(), "Server startup failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    panic!("Server did not become ready");
}

async fn stop(address: &str, token: &str, task: tokio::task::JoinHandle<atelier_app::Result<()>>) {
    assert!(
        reqwest::Client::new()
            .post(format!("{address}/api/prototype/stop"))
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn next_event(response: &mut reqwest::Response, buffered: &mut String) -> String {
    tokio::time::timeout(Duration::from_secs(17), async {
        loop {
            if let Some(end) = buffered.find("\n\n") {
                return buffered.drain(..end + 2).collect();
            }
            let chunk = response.chunk().await.unwrap().expect("event stream ended");
            buffered.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    })
    .await
    .expect("event did not arrive")
}

#[tokio::test]
async fn local_time_routes_serialize_timer_starts_preserve_updates_and_close_cleanly() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let request = |method, path: &str| {
        client
            .request(method, format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let card: Value = request(reqwest::Method::POST, "/api/cards")
        .json(&json!({"title":"Synthetic timer"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let fields = json!({"cardId":card["id"], "seconds":60, "date":"2026-10-22", "startTime":"09:00:00", "description":"Work", "start":true});
    let (first, second) = tokio::join!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send(),
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let (created, refused) = if first.status().is_success() {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(created.status(), 200);
    assert_eq!(refused.status(), 400);
    let created: Value = created.json().await.unwrap();
    assert_eq!(created["seconds"], 0);
    let card_id = card["id"].as_str().unwrap();
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":true}))
        .send()
        .await
        .unwrap()
        .status(),
        409
    );
    assert_eq!(
        request(reqwest::Method::DELETE, &format!("/api/cards/{card_id}"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        request(reqwest::Method::DELETE, &format!("/api/time/{id}"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let stopped: Value = request(reqwest::Method::POST, &format!("/api/time/{id}/stop"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(stopped.get("runningSince").is_none());
    let mut update = fields.clone();
    update["seconds"] = json!(90);
    update["description"] = json!(" Modified ");
    let modified: Value = request(reqwest::Method::PATCH, &format!("/api/time/{id}"))
        .json(&update)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(modified["seconds"], 90);
    assert_eq!(modified["description"], "Modified");
    assert_eq!(modified["interrupted"], false);
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":"true"}))
        .send()
        .await
        .unwrap()
        .status(),
        400
    );
    let archived: Value = request(
        reqwest::Method::POST,
        &format!("/api/cards/{card_id}/archive"),
    )
    .json(&json!({"archived":true}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(archived["archived"], true);
    assert_eq!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":false}))
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    let mut events = request(reqwest::Method::GET, "/api/events")
        .send()
        .await
        .unwrap();
    let mut buffered = String::new();
    assert!(
        next_event(&mut events, &mut buffered)
            .await
            .contains("data: connected")
    );
    assert_eq!(
        request(reqwest::Method::POST, &format!("/api/time/{id}/export"))
            .json(&json!({"confirmed":true}))
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    assert_eq!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        next_event(&mut events, &mut buffered).await,
        "data: changed\n\n"
    );
    assert_eq!(
        next_event(&mut events, &mut buffered).await,
        "data: changed\n\n"
    );
    let ticking: Value = request(reqwest::Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(ticking["times"][0]["seconds"].as_f64().unwrap() >= 1.0);
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["times"].as_array().unwrap().len(), 2);
    assert!(
        board["times"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry.get("runningSince").is_none())
    );
    assert_eq!(board["times"][1]["description"], "Modified");
    assert_eq!(
        client
            .delete(format!("{address}/api/cards/{card_id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .delete(format!("{address}/api/time/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let other = board["times"][0]["id"].as_str().unwrap();
    assert_eq!(
        client
            .delete(format!("{address}/api/time/{other}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let removed: Value = client
        .delete(format!("{address}/api/cards/{card_id}"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(removed, json!({"ok":true}));
    assert_eq!(
        client
            .post(format!("{address}/api/cards/{card_id}/archive"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"archived":true}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn complete_creation_uses_templates_projects_native_paths_and_atomic_storage() {
    use atelier_app::infrastructure::store::Documents;
    use atelier_core::{
        contracts::Board,
        document::{Document, JsonString},
    };
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let repositories = tempfile::tempdir().unwrap();
    let repo = repositories.path().join("dépôt 🔧");
    std::fs::create_dir(&repo).unwrap();
    let file = repositories.path().join("file");
    std::fs::write(&file, "synthetic").unwrap();
    {
        let root = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&root.path.join("kanban.sqlite")).unwrap();
        let repo = repo.clone();
        database.call(move |store| {
            let mut settings: Document = serde_json::from_str(r#"{"agent":"codex","unknown":"\ud800","taskTemplates":[{"id":"template","description":"\ud800","instructions":" inherited ","mode":"analyze","checklist":[{"text":" \udc00 ","done":false}]}]}"#)?;
            settings.set("repo", &repo)?;
            store.save_settings(&settings)?;
            store.put(Documents::Projects, &json!({"id":"project", "repo":repo, "agent":"claude", "unknown":"keep"}))?;
            Ok(())
        }).await.unwrap();
        database.close().await.unwrap();
        let injection = rusqlite::Connection::open(root.path.join("kanban.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_creation BEFORE INSERT ON cards WHEN json_extract(NEW.data, '$.title')='fail' BEGIN SELECT RAISE(ABORT, 'synthetic private database detail'); END;").unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let create = |path: &str| {
        client
            .post(format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let first = create("/api/cards").json(&json!({"title":"Inherited", "templateId":"template", "status":"doing", "unknown":"discard"}));
    let second = create("/api/cards").json(&json!({"title":"Explicit", "repo":repo, "agent":"codex", "model":" model/v1 ", "effort":"high", "checklist":[{"text":" criterion ","done":true}]}));
    let (first, second) = tokio::join!(first.send(), second.send());
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 200);
    let first: Card = first.json().await.unwrap();
    let second: Card = second.json().await.unwrap();
    assert_ne!(first.id, second.id);
    assert!(first.fields.string_equals("agent", "claude").unwrap());
    assert!(first.fields.string_equals("status", "doing").unwrap());
    assert_eq!(
        first.fields.get::<JsonString>("description").unwrap(),
        Some(JsonString(vec![0xd800]))
    );
    assert!(first.fields.raw("unknown").is_none());
    assert!(second.fields.string_equals("model", "model/v1").unwrap());
    let resolved = second.fields.get::<String>("repo").unwrap().unwrap();
    assert!(!resolved.starts_with(r"\\?\"));
    assert_eq!(
        std::fs::canonicalize(&resolved).unwrap(),
        std::fs::canonicalize(&repo).unwrap()
    );
    let idea = create("/api/brainstorms").json(&json!({"title":"Idea", "templateId":"template", "mode":"invalid", "status":"invalid", "description":"", "checklist":[]})).send().await.unwrap();
    assert_eq!(idea.status(), 200);
    let idea: Card = idea.json().await.unwrap();
    for (key, value) in [
        ("mode", "analyze"),
        ("status", "todo"),
        ("repo", ""),
        ("description", ""),
    ] {
        assert!(idea.fields.string_equals(key, value).unwrap());
    }
    assert_eq!(
        idea.fields.get::<bool>("brainstormSession").unwrap(),
        Some(true)
    );
    for body in [
        json!({"title":"Bad", "repo":file}),
        json!({"title":"Bad", "repo":repo.join("missing")}),
        json!({"title":"Bad", "repo":"x\u{0}y"}),
        json!({"title":"Bad", "templateId":"missing"}),
        json!({"title":"Bad", "model":false}),
    ] {
        assert_eq!(
            create("/api/cards")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let failed = create("/api/cards")
        .json(&json!({"title":"fail"}))
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    assert_eq!(
        failed.json::<Value>().await.unwrap(),
        json!({"error":"Création de la carte impossible."})
    );
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board.cards.len(), 3);
    let saved = board.cards.iter().find(|card| card.id == first.id).unwrap();
    let criteria = saved
        .fields
        .get::<Vec<Document>>("checklist")
        .unwrap()
        .unwrap();
    assert_eq!(
        criteria[0].get::<JsonString>("text").unwrap(),
        Some(JsonString(vec![0xdc00]))
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn template_routes_serialize_the_limit_and_preserve_it_across_restart() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let card: Value = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Template source"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let responses = futures_util::future::join_all((0..55).map(|index| {
        client
            .post(format!("{address}/api/templates"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"name":format!("Template {index}"), "cardId":card["id"]}))
            .send()
    }))
    .await;
    let mut ids = Vec::new();
    for response in responses {
        let response = response.unwrap();
        if response.status().is_success() {
            let template: Value = response.json().await.unwrap();
            ids.push(template["id"].as_str().unwrap().to_owned());
            assert_eq!(template["mode"], "edit");
        } else {
            assert_eq!(response.status(), 400);
        }
    }
    assert_eq!(ids.len(), 50);
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    for id in [&ids[0], &"missing".to_owned()] {
        assert_eq!(
            client
                .delete(format!("{address}/api/templates/{id}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }
    for expected in [200, 400] {
        assert_eq!(
            client
                .post(format!("{address}/api/templates"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({"name":"Again", "cardId":card["id"]}))
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/templates"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"name":"Missing", "cardId":"missing"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn events_authenticate_notify_commits_reconnect_and_end_on_shutdown() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let url = format!("{address}/api/events?session={token}");
    assert_eq!(
        client
            .get(&url)
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(&url)
            .header("origin", "https://foreign.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let mut buffered = String::new();
    let connected = next_event(&mut response, &mut buffered).await;
    assert!(connected.contains("retry: 1000\n"));
    assert!(connected.contains("data: connected\n"));
    assert_eq!(
        next_event(&mut response, &mut buffered).await,
        ": heartbeat\n\n"
    );
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"title":""}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), response.chunk())
            .await
            .is_err()
    );
    let created: Value = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"synthetic event"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        next_event(&mut response, &mut buffered).await,
        "data: changed\n\n"
    );
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["cards"][0]["id"], created["id"]);
    drop(response);
    let mut response = client
        .get(format!("{address}/api/events"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    buffered.clear();
    assert!(
        next_event(&mut response, &mut buffered)
            .await
            .contains("data: connected\n")
    );
    stop(&address, &token, task).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), response.chunk())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn authenticated_http_preserves_tasks_after_close_and_rotates_sessions() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>Fixture</title>",
    )
    .unwrap();
    std::fs::write(assets.path().join("local-session"), "must not be served").unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    for path in [
        "/api/state",
        "/api/health",
        "/api/events",
        "/api/state?session=ignored",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for (header, value) in [
        ("host", "foreign.example"),
        ("origin", "https://foreign.example"),
    ] {
        assert_eq!(
            client
                .get(format!("{address}/api/state"))
                .bearer_auth(&token)
                .header(header, value)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    let challenge = "a".repeat(64);
    let health: Value = client
        .get(format!("{address}/api/health?challenge={challenge}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let port = address.rsplit(':').next().unwrap().parse().unwrap();
    assert_eq!(
        health,
        json!({"proof": session::proof(&token, &challenge, port).unwrap()})
    );
    let index = client.get(&address).send().await.unwrap();
    assert_eq!(index.headers()["content-security-policy"], http::CSP);
    assert!(!http::CSP.contains("'unsafe-eval'"));
    assert!(!http::CSP.contains("'unsafe-inline'"));
    for path in [
        "/local-session",
        "/data/local-session",
        "/Cargo.toml",
        "/api/missing",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .json(&json!({"title":"Refusée"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for body in [
        "[]".to_owned(),
        "null".to_owned(),
        "{".to_owned(),
        "{\"title\":\" \"}".to_owned(),
    ] {
        assert_eq!(
            client
                .post(format!("{address}/api/cards"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"title":"a".repeat(100_001)}))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let created = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":" Tâche 🔧 <script> "}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let card: Value = created.json().await.unwrap();
    assert_eq!(card["title"], "Tâche 🔧 <script>");
    assert_eq!(card["createdAt"].as_str().unwrap().len(), 24);
    stop(&address, &token, task).await;
    let (address, next_token, task) = start(data.path(), assets.path()).await;
    assert!(!session::same_secret(&token, &next_token));
    assert_eq!(
        client
            .get(format!("{address}/api/state"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&next_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board, json!({"cards":[card], "times":[]}));
    stop(&address, &next_token, task).await;
}

#[tokio::test]
async fn http_and_restart_preserve_isolated_utf16_and_unknown_json_keys() {
    use atelier_core::{
        contracts::Board,
        document::{Document, JsonString},
    };
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>Fixture</title>",
    )
    .unwrap();
    let (address, token, server) = start(data.path(), assets.path()).await;
    let response = reqwest::Client::new()
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .header("content-type", "application/json")
        .body(r#"{"title":"\ufeff\ud83d\u00a0"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut card: Card = response.json().await.unwrap();
    assert_eq!(card.title, JsonString(vec![0xd83d]));
    stop(&address, &token, server).await;
    card.fields =
        serde_json::from_str::<Document>(r#"{"\ud800":{"unknown":"\udc00"},"nullable":null}"#)
            .unwrap();
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    database.put_card(card).await.unwrap();
    database.close().await.unwrap();
    let (address, token, server) = start(data.path(), assets.path()).await;
    let response = reqwest::Client::new()
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(raw.contains(r#""\ud800":{"unknown":"\udc00"}"#));
    let board: Board = serde_json::from_str(&raw).unwrap();
    assert_eq!(board.cards[0].title, JsonString(vec![0xd83d]));
    let client = reqwest::Client::new();
    let duplicate = || {
        client
            .post(format!(
                "{address}/api/cards/{}/duplicate",
                board.cards[0].id
            ))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
    };
    let (first, second) = tokio::join!(duplicate().send(), duplicate().send());
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 200);
    let first: Card = first.json().await.unwrap();
    let second: Card = second.json().await.unwrap();
    assert_ne!(first.id, second.id);
    let mut copied_title = JsonString::from("Copie — ");
    copied_title.0.push(0xd83d);
    assert_eq!(first.title, copied_title);
    assert_eq!(second.title, copied_title);
    assert!(first.fields.string_equals("status", "todo").unwrap());
    assert!(first.fields.raw("nullable").is_none());
    let missing = client
        .post(format!("{address}/api/cards/missing/duplicate"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert_eq!(
        missing.json::<Value>().await.unwrap(),
        json!({"error":"Carte introuvable."})
    );
    stop(&address, &token, server).await;
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let saved = database.cards().await.unwrap();
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[2].title, JsonString(vec![0xd83d]));
    assert!(saved[2].fields.raw("nullable").is_some());
    database.close().await.unwrap();
}

#[tokio::test]
async fn database_preserves_unknown_fields_null_and_row_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.sqlite");
    let database = Database::open(&path).unwrap();
    let first: Card =
        serde_json::from_str(r#"{"id":"1", "title":"é", "extra":{"nullable":null}}"#).unwrap();
    let second: Card = serde_json::from_str(r#"{"id":"2", "title":"two"}"#).unwrap();
    database.put_card(first.clone()).await.unwrap();
    database.put_card(second.clone()).await.unwrap();
    let mut updated = first;
    updated.title = "changed".into();
    database.put_card(updated.clone()).await.unwrap();
    database.close().await.unwrap();
    let reopened = Database::open(&path).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.cards().await.unwrap()).unwrap(),
        serde_json::to_value(vec![second, updated]).unwrap()
    );
    reopened.close().await.unwrap();
}

#[test]
fn prototype_refuses_existing_data_and_locks_aliases_until_closed() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("kanban.sqlite"), "untouched").unwrap();
    assert!(PrototypeDirectory::open(directory.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("kanban.sqlite")).unwrap(),
        "untouched"
    );
    let empty = tempfile::tempdir().unwrap();
    let first = PrototypeDirectory::open(empty.path()).unwrap();
    assert!(PrototypeDirectory::open(&empty.path().join(".")).is_err());
    drop(first);
    assert!(PrototypeDirectory::open(empty.path()).is_ok());
}

#[tokio::test]
async fn dropping_database_drains_an_accepted_write_after_caller_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cancelled.sqlite");
    let database = Database::open(&path).unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let mut operation = Box::pin(database.call(move |store| {
        started.send(()).unwrap();
        waiting.recv().unwrap();
        store.save_settings(&json!({"accepted":true}))
    }));
    tokio::select! {
        _ = ready => {},
        _ = &mut operation => panic!("Write finished before release"),
    }
    drop(operation);
    release.send(()).unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    assert_eq!(
        reopened
            .call(|store| store.settings::<serde_json::Value>())
            .await
            .unwrap(),
        json!({"accepted":true})
    );
    reopened.close().await.unwrap();
}

#[test]
fn session_tokens_and_constant_time_comparison_reject_malformed_values() {
    // Generated with the existing Node sessionProof, not with this Rust implementation.
    assert_eq!(
        session::proof(&"a".repeat(64), &"b".repeat(64), 4318).unwrap(),
        "1ae8821520411244274c996fe46f3a9ee07c312cc03462a73e34a72c949d7bfb"
    );
    let directory = tempfile::tempdir().unwrap();
    let token = session::new_token().unwrap();
    session::write(directory.path(), &token).unwrap();
    assert!(session::same_secret(
        &token,
        &session::read(directory.path()).unwrap()
    ));
    assert!(!session::same_secret(
        &token,
        &session::new_token().unwrap()
    ));
    assert!(!session::same_secret(&token, &"é".repeat(64)));
    session::write(directory.path(), "invalid").unwrap();
    assert!(session::read(directory.path()).is_err());
}

#[cfg(windows)]
#[test]
fn dpapi_is_compatible_with_powershell_in_both_directions() {
    use std::os::windows::process::CommandExt;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    fn powershell(input: &str, decrypt: bool) -> String {
        let operation = if decrypt {
            "$secure = ConvertTo-SecureString -String $value; [Console]::Write(([System.Net.NetworkCredential]::new('', $secure)).Password)"
        } else {
            "$secure = ConvertTo-SecureString -String $value -AsPlainText -Force; [Console]::Write((ConvertFrom-SecureString -SecureString $secure))"
        };
        let script = format!(
            "$ErrorActionPreference='Stop'; Import-Module ([System.Reflection.Assembly]::LoadWithPartialName('Microsoft.PowerShell.Security').Location); [Console]::InputEncoding=New-Object System.Text.UTF8Encoding($false); [Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false); $value=[Console]::In.ReadToEnd(); {operation}"
        );
        let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut child = Command::new(executable)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "PowerShell DPAPI test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    let plaintext = "Secret synthétique 🔧 uniquement";
    let encrypted = session::protect(plaintext, false).unwrap();
    assert_eq!(powershell(&encrypted, true), plaintext);
    assert_eq!(
        session::protect(&powershell(plaintext, false), true).unwrap(),
        plaintext
    );
    assert!(session::protect("bad", true).is_err());
    assert!(session::protect("00", true).is_err());
}
