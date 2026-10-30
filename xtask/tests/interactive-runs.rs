use atelier_app::infrastructure::{
    command_runs::{CommandRuns, RunInput},
    database::Database,
    files::Repository,
    prototype::PrototypeDirectory,
    store::Documents,
};
use atelier_core::{
    application::agent_protocol::{Kind, Protocol, ProtocolConfig},
    contracts::{AgentAnswer, AgentInteraction},
    document::Document,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

async fn interaction(database: &Database, questions: bool) -> AgentInteraction {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let requests = database
            .call(|store| {
                Ok(store
                    .document::<Document>(Documents::Runs, "run")?
                    .unwrap()
                    .get::<Vec<AgentInteraction>>("interactions")?
                    .unwrap_or_default())
            })
            .await
            .unwrap();
        if let Some(request) = requests
            .into_iter()
            .find(|request| request.questions.is_empty() != questions)
        {
            return request;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Synthetic agent did not request input"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn fake_clis_ask_wait_reply_finish_mask_secrets_and_reject_stale_answers_on_the_common_engine()
 {
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    for kind in [Kind::Codex, Kind::Claude, Kind::Acp] {
        for mode in ["success", "deny", "cancel", "malformed", "missing"] {
            if kind == Kind::Acp && mode != "success" {
                continue;
            }
            let temporary = tempfile::tempdir().unwrap();
            let prototype = PrototypeDirectory::open(temporary.path()).unwrap();
            let path = prototype.path.join("repositories/synthetic");
            std::fs::create_dir_all(path.join(".git")).unwrap();
            std::fs::write(
                path.join(".agent-run-fixture"),
                if mode == "malformed" {
                    "atelier-r03-synthetic-malformed"
                } else {
                    "atelier-r03-synthetic-only"
                },
            )
            .unwrap();
            let root = prototype.opened_directory().unwrap();
            let selected = Repository::synthetic(
                &root,
                std::slice::from_ref(&prototype.path),
                path.to_str().unwrap(),
            )
            .unwrap();
            let database = Arc::new(prototype.database().unwrap());
            database
                .call(|store| {
                    store.put(
                        Documents::Cards,
                        &json!({"id":"card","title":"Synthetic agent task"}),
                    )
                })
                .await
                .unwrap();
            let mut record: Document =
                serde_json::from_str(r#"{"id":"run","cardId":"card","status":"starting"}"#)
                    .unwrap();
            record.set("repo", &path.to_str().unwrap()).unwrap();
            let mut command = tokio::process::Command::new(if mode == "missing" {
                path.join("missing-synthetic-agent.exe")
            } else {
                executable.to_owned()
            });
            command
                .arg(if kind == Kind::Acp {
                    "acp"
                } else if kind == Kind::Codex {
                    "app-server"
                } else {
                    "-p"
                })
                .current_dir(&path)
                .env_clear();
            let protocol = Protocol::new(ProtocolConfig {
                kind,
                edit: true,
                repo: path.to_str().unwrap().into(),
                prompt: "Synthetic prompt".into(),
                model: String::new(),
                effort: String::new(),
                api_key: if kind == Kind::Codex {
                    "synthetic-run-key".into()
                } else {
                    String::new()
                },
                secrets: Vec::new(),
            });
            let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
            manager
                .start(
                    database.clone(),
                    record,
                    command,
                    RunInput::Agent(Box::new(protocol)),
                    atelier_app::infrastructure::command_runs::RunDirectory::Worktree(selected),
                )
                .await
                .unwrap();
            if !["malformed", "missing"].contains(&mode) {
                let approval = interaction(&database, false).await;
                assert!(approval.can_allow);
                assert_eq!(manager.active_cards(), vec!["card"]);
                if mode == "cancel" {
                    manager.stop("run").await.unwrap();
                } else {
                    let answer = AgentAnswer {
                        request_id: approval.id,
                        decision: if mode == "deny" { "deny" } else { "allow" }.into(),
                        answers: BTreeMap::new(),
                    };
                    manager.reply("run", answer.clone()).await.unwrap();
                    assert!(manager.reply("run", answer).await.is_err());
                    if kind != Kind::Acp {
                        let question = interaction(&database, true).await;
                        assert!(question.questions[0].is_secret);
                        let invalid = AgentAnswer {
                            request_id: question.id.clone(),
                            decision: "answer".into(),
                            answers: BTreeMap::new(),
                        };
                        assert!(manager.reply("run", invalid.clone()).await.is_err());
                        let mut valid = invalid;
                        valid
                            .answers
                            .insert("0".into(), vec!["synthetic-private-answer".into()]);
                        manager.reply("run", valid).await.unwrap();
                    }
                }
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            let run = loop {
                let run = database
                    .call(|store| Ok(store.document::<Document>(Documents::Runs, "run")?.unwrap()))
                    .await
                    .unwrap();
                if !atelier_app::infrastructure::runs::active(&run).unwrap() {
                    break run;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "Synthetic agent did not finish: {}/{mode}, status={:?}, attention={:?}",
                    if kind == Kind::Codex {
                        "codex"
                    } else {
                        "claude"
                    },
                    run.get::<String>("status").unwrap(),
                    run.get::<String>("attention").unwrap()
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            assert!(
                run.string_equals(
                    "status",
                    match mode {
                        "success" => "completed",
                        "cancel" => "cancelled",
                        _ => "failed",
                    }
                )
                .unwrap()
            );
            assert!(run.raw("interactions").is_none());
            assert!(run.raw("liveText").is_none());
            let logs = database.call(|store| store.logs("run", 0)).await.unwrap();
            let public = format!(
                "{} {}",
                serde_json::to_string(&run).unwrap(),
                serde_json::to_string(&logs).unwrap()
            );
            for secret in [
                "synthetic-run-key",
                "synthetic-private-answer",
                "private-synthetic-reasoning",
            ] {
                assert!(!public.contains(secret));
            }
            if mode == "success" {
                assert!(public.contains("Public synthetic"));
                if kind != Kind::Acp {
                    assert_eq!(
                        run.get::<serde_json::Value>("usage").unwrap().unwrap()["inputTokens"],
                        5
                    );
                }
            }
            manager.close().await.unwrap();
            database.close().await.unwrap();
        }
    }
}
