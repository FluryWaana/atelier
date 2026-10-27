use atelier_app::infrastructure::extensions::Executable;
use atelier_core::application::extensions::{Manifest, Method, PROTOCOL, Request, Status};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
static EXTENSION_FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[path = "support/windows.rs"]
#[cfg(windows)]
mod windows;

#[tokio::test]
async fn a_lost_process_reply_is_reconciled_without_replaying_its_synthetic_effect() {
    lost_reply(Method::Action).await;
}

#[tokio::test]
async fn a_lost_time_send_is_reconciled_without_sending_the_time_twice() {
    lost_reply(Method::TimeSend).await;
}

async fn lost_reply(method: Method) {
    let _fixture = EXTENSION_FIXTURE.lock().await;
    use atelier_app::infrastructure::{
        connector_effects as effects, connector_time, connectors, database::Database,
        store::Documents,
    };
    use atelier_core::{
        application::extensions::{RemotePreview, Report},
        contracts::ForgeWriteState as Phase,
        document::Document,
    };
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(
        fixture.path().join(".extension-fixture"),
        "atelier-r04-synthetic-only",
    )
    .unwrap();
    let (binary, manifest) = installed().await;
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    let settings: Document = serde_json::from_str(&json!({"connectorRevision":1,"processConnectors":[{"manifest":manifest,"binary":binary,"revision":1,"values":{"fault":"lost-write","fixture-root":fixture.path()},"report":null}]}).to_string()).unwrap();
    let prepared = db
        .call(move |store| {
            store.save_settings(&settings)?;
            let input = if method == Method::TimeSend {
                let card: Document = serde_json::from_str(&json!({"id":"22222222-2222-4222-8222-222222222222","title":"Synthetic timed task"}).to_string())?;
                let entry: Document = serde_json::from_str(&json!({"id":"11111111-1111-4111-8111-111111111111","cardId":"22222222-2222-4222-8222-222222222222","seconds":120,"date":"2026-10-26","startTime":"10:00:00","description":"Synthetic human work","exportState":"draft"}).to_string())?;
                store.put(Documents::Cards, &card)?;
                store.put(Documents::Times, &entry)?;
                let mut selection = json!({"expected":entry,"expectedCard":card});
                let source = connector_time::preview_input(store, &selection)?;
                connectors::record(store,"synthetic-extension",1,Report{id:"synthetic-time-report".into(),method:Method::TimePreview,action:None,message:"Synthetic time preview".into(),output:json!({"source":source,"instance":"https://synthetic.example.test/time","account":"synthetic-time-account","reference":"SYN-1"}),observed_at:time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?})?;
                selection["reportId"] = json!("synthetic-time-report");
                selection
            } else { Value::Null };
            effects::preview(
                store,
                "synthetic-extension",
                &RemotePreview {
                    expected_revision: 1,
                    method,
                    input,
                    action: if method == Method::Action {"synthetic-write".into()} else {String::new()},
                },
            )
        })
        .await
        .unwrap();
    let proposal = prepared.input.clone();
    let id = prepared.id;
    let extension = if method == Method::TimeSend {
        "time-effect"
    } else {
        "effect"
    };
    let marker = fixture.path().join(format!("{id}.{extension}"));
    assert!(!marker.exists());
    let selected = id.clone();
    let (profile, request) = db
        .call(move |store| {
            effects::prepare(
                store,
                "synthetic-extension",
                &selected,
                1,
                1,
                false,
                &mut |value, _| Ok(value.into()),
            )
        })
        .await
        .unwrap();
    let nonce = request.request_id.clone();
    let selected = id.clone();
    db.call(move |store| {
        let operation = effects::sending(store, "synthetic-extension", &selected, 1, 1, &nonce)?;
        if method == Method::TimeSend {
            assert!(
                store
                    .document::<Document>(Documents::Times, "11111111-1111-4111-8111-111111111111")?
                    .unwrap()
                    .string_equals("exportState", "sending")?
            );
        }
        Ok(operation)
    })
    .await
    .unwrap();
    assert!(
        Executable::open(
            PathBuf::from(&profile.binary.path),
            Some(profile.binary.sha256)
        )
        .await
        .unwrap()
        .prepare(&profile.manifest, request, Duration::from_secs(15))
        .unwrap()
        .dispatch()
        .await
        .is_err()
    );
    let marker_bytes = std::fs::read(&marker).unwrap();
    if method == Method::TimeSend {
        assert_eq!(
            serde_json::from_slice::<Value>(&marker_bytes).unwrap(),
            proposal
        );
    } else {
        assert_eq!(marker_bytes, b"one synthetic effect");
    }
    let modified = std::fs::metadata(&marker).unwrap().modified().unwrap();
    let selected = id.clone();
    let (profile, request, version) = db
        .call(move |store| {
            effects::recover(store)?;
            let uncertain = effects::current(store, "synthetic-extension", &selected)?;
            assert_eq!(uncertain.state, Phase::Uncertain);
            let reconciliation =
                format!("/api/extensions/synthetic-extension/operations/{selected}/reconcile");
            assert_eq!(
                effects::reconciles_time(store, &reconciliation)?,
                method == Method::TimeSend
            );
            for wrong in [
                reconciliation.replace("/reconcile", "/send"),
                reconciliation.replace("synthetic-extension/", "other-module/"),
                format!("{reconciliation}/extra"),
            ] {
                assert!(!effects::reconciles_time(store, &wrong)?);
            }
            if method == Method::TimeSend {
                assert!(
                    store
                        .document::<Document>(
                            Documents::Times,
                            "11111111-1111-4111-8111-111111111111"
                        )?
                        .unwrap()
                        .string_equals("exportState", "uncertain")?
                );
            }
            assert!(
                effects::prepare(
                    store,
                    "synthetic-extension",
                    &selected,
                    uncertain.version,
                    1,
                    false,
                    &mut |value, _| Ok(value.into())
                )
                .is_err()
            );
            let (profile, request) = effects::prepare(
                store,
                "synthetic-extension",
                &selected,
                uncertain.version,
                1,
                true,
                &mut |value, _| Ok(value.into()),
            )?;
            let observing = effects::observing(
                store,
                "synthetic-extension",
                &selected,
                uncertain.version,
                1,
                &request.request_id,
            )?;
            Ok((profile, request, observing.version))
        })
        .await
        .unwrap();
    assert_eq!(
        request.method,
        if method == Method::TimeSend {
            Method::TimeReconcile
        } else {
            Method::Reconcile
        }
    );
    assert_eq!(request.operation_id.as_deref(), Some(id.as_str()));
    let reply = Executable::open(
        PathBuf::from(&profile.binary.path),
        Some(profile.binary.sha256),
    )
    .await
    .unwrap()
    .prepare(&profile.manifest, request, Duration::from_secs(15))
    .unwrap()
    .dispatch()
    .await
    .unwrap();
    db.call(move |store| {
        let observed = effects::record(store, "synthetic-extension", &id, version, 1, reply, true)?;
        assert_eq!(observed.state, Phase::Reconciled);
        assert!(!effects::reconciles_time(
            store,
            &format!("/api/extensions/synthetic-extension/operations/{id}/reconcile")
        )?);
        if method == Method::TimeSend {
            assert!(
                store
                    .document::<Document>(Documents::Times, "11111111-1111-4111-8111-111111111111")?
                    .unwrap()
                    .string_equals("exportState", "exported")?
            );
        }
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(&marker).unwrap(), marker_bytes);
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        std::fs::read_dir(fixture.path())
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|value| value == extension))
            .count(),
        1
    );
    db.close().await.unwrap();
}

fn request(fault: &str) -> Request {
    Request {
        protocol: PROTOCOL.into(),
        request_id: format!("request-{fault}"),
        module: "synthetic-extension".into(),
        method: Method::Check,
        operation_id: None,
        action: None,
        settings: BTreeMap::from([("fault".into(), Value::String(fault.into()))]),
        secrets: BTreeMap::from([("token".into(), "synthetic-extension-private-token".into())]),
        input: Value::Null,
    }
}
async fn installed() -> (atelier_core::application::extensions::Binary, Manifest) {
    Executable::open(PathBuf::from(env!("CARGO_BIN_EXE_native-fixture")), None)
        .await
        .unwrap()
        .describe("synthetic-extension")
        .await
        .unwrap()
}

#[tokio::test]
async fn pinned_processes_keep_credentials_in_stdin_and_reject_untrusted_frames() {
    let _fixture = EXTENSION_FIXTURE.lock().await;
    let (binary, manifest) = installed().await;
    let fixture = Executable::open(PathBuf::from(&binary.path), None)
        .await
        .unwrap();
    assert!(
        fixture
            .prepare(&manifest, request("success"), Duration::from_secs(15))
            .is_err()
    );
    assert!(
        Executable::open(PathBuf::from(&binary.path), Some("0".repeat(64)))
            .await
            .is_err()
    );
    for fault in [
        "success",
        "secret",
        "secret-key",
        "stderr",
        "identity",
        "extra",
        "partial",
        "flood",
    ] {
        let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256.clone()))
            .await
            .unwrap();
        let result = executable
            .prepare(&manifest, request(fault), Duration::from_secs(15))
            .unwrap()
            .dispatch()
            .await;
        if fault == "success" {
            let reply = result.unwrap();
            assert_eq!(reply.status, Status::Ok);
            assert_eq!(reply.output["privateHome"], true);
        } else {
            let error = result.unwrap_err().to_string();
            assert!(
                !error.contains("synthetic-extension-private-token"),
                "{fault}"
            );
        }
    }
    let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256.clone()))
        .await
        .unwrap();
    let mut public_secret = request("success");
    public_secret
        .settings
        .insert("token".into(), json!("must-never-be-public"));
    assert!(
        executable
            .prepare(&manifest, public_secret, Duration::from_secs(15))
            .is_err()
    );
    for path in [
        "relative.exe",
        r"\\synthetic-server\share\module.exe",
        "C:/synthetic-module.mjs",
    ] {
        assert!(Executable::open(PathBuf::from(path), None).await.is_err());
    }
}

#[tokio::test]
#[cfg(windows)]
async fn timeout_stops_the_extension_and_its_descendant_before_returning() {
    let _fixture = EXTENSION_FIXTURE.lock().await;
    let (binary, manifest) = installed().await;
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(
        fixture.path().join(".extension-fixture"),
        "atelier-r04-synthetic-only",
    )
    .unwrap();
    let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256))
        .await
        .unwrap();
    let mut request = request("hang");
    request.input = json!({"fixtureRoot":fixture.path()});
    let prepared = executable
        .prepare(&manifest, request, Duration::from_secs(3))
        .unwrap();
    let task = tokio::spawn(async move { prepared.dispatch().await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Ok(pid) = std::fs::read_to_string(fixture.path().join("extension-child.pid")) {
            break pid.parse::<u32>().unwrap();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Synthetic descendant never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let process = windows::open_process(pid).unwrap();
    assert!(windows::creation_time(&process).unwrap() > 0);
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        windows::wait(&process, 1000),
        windows_sys::Win32::Foundation::WAIT_OBJECT_0
    );
}

#[tokio::test]
#[cfg(windows)]
async fn a_pinned_synthetic_image_cannot_be_replaced_until_its_call_is_released() {
    let _fixture = EXTENSION_FIXTURE.lock().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-extension.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &path).unwrap();
    let executable = Executable::open(path.clone(), None).await.unwrap();
    let binary = executable.binary();
    assert!(std::fs::write(&path, b"replacement must fail while pinned").is_err());
    assert!(
        Executable::open(path.clone(), Some(binary.sha256.clone()))
            .await
            .is_ok()
    );
    drop(executable);
    std::fs::write(&path, b"synthetic replacement after release").unwrap();
    assert!(Executable::open(path, Some(binary.sha256)).await.is_err());
}
