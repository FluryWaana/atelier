use atelier_app::infrastructure::{
    backups::BackupDirectory,
    database::Database,
    forge::{self, Forge},
    store::Documents,
};
use atelier_core::{
    contracts::{ForgeConfiguration, ForgeKind, ForgeUpdate},
    document::Document,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    fault: &'static str,
}
async fn remote(
    State(remote): State<Remote>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    assert_eq!(method, Method::GET);
    let authorization = if remote.kind == ForgeKind::Azure {
        format!("Basic {}", STANDARD.encode(":synthetic-token"))
    } else {
        "Bearer synthetic-token".into()
    };
    assert_eq!(
        headers[header::AUTHORIZATION].to_str().unwrap(),
        authorization.as_str()
    );
    let path = uri.path();
    let sha = "a".repeat(40);
    let target = "b".repeat(40);
    let mut row = match remote.kind {
        ForgeKind::Github => {
            assert_eq!(headers["x-github-api-version"], "2022-11-28");
            assert_eq!(path, "/api/v3/repos/owner/repo/pulls/7");
            json!({"number":7,"title":"Synthetic PR é🦀","state":"open","merged":false,"draft":true,"head":{"ref":"feature/synthetic","sha":sha,"repo":{"full_name":if remote.fault=="fork"{"other/repo"}else{"owner/repo"}}},"base":{"ref":"main","sha":target,"repo":{"full_name":"owner/repo"}},"html_url":"https://untrusted.example.test/ignored"})
        }
        ForgeKind::Gitlab => {
            assert!(path.starts_with("/gitlab/api/v4/projects/group%2Fnested%2Frepo"));
            if !path.ends_with("/merge_requests/7") {
                assert_eq!(path, "/gitlab/api/v4/projects/group%2Fnested%2Frepo");
                return Json(json!({"id":1,"path_with_namespace":"group/nested/repo"}))
                    .into_response();
            }
            json!({"iid":7,"title":"Synthetic MR é🦀","state":"opened","draft":true,"source_branch":"feature/synthetic","target_branch":"main","source_project_id":if remote.fault=="fork"{2}else{1},"target_project_id":1,"diff_refs":{"head_sha":sha,"start_sha":target},"web_url":"https://untrusted.example.test/ignored"})
        }
        ForgeKind::Azure => {
            assert_eq!(uri.query(), Some("api-version=7.1"));
            assert!(path.starts_with("/org/project/_apis/git/repositories/repo"));
            if !path.ends_with("/pullrequests/7") {
                assert_eq!(path, "/org/project/_apis/git/repositories/repo");
                return Json(json!({"id":"synthetic-repository"})).into_response();
            }
            json!({"pullRequestId":7,"title":"Synthetic PR é🦀","status":"active","isDraft":true,"sourceRefName":"refs/heads/feature/synthetic","targetRefName":"refs/heads/main","repository":{"id":"synthetic-repository"},"lastMergeSourceCommit":{"commitId":sha},"lastMergeTargetCommit":{"commitId":target},"forkSource":if remote.fault=="fork"{json!({"repository":{"id":"other"}})}else{Value::Null},"url":"https://untrusted.example.test/ignored"})
        }
    };
    match remote.fault {
        "secret" => row["title"] = json!("synthetic-token"),
        "identity" => {
            row["number"] = json!(8);
            row["iid"] = json!(8);
            row["pullRequestId"] = json!(8);
        }
        "commit" => {
            row["head"]["sha"] = json!("not-a-commit");
            row["diff_refs"]["head_sha"] = json!("not-a-commit");
            row["lastMergeSourceCommit"]["commitId"] = json!("not-a-commit");
        }
        "missing-commits" => {
            row["head"]["sha"] = Value::Null;
            row["base"]["sha"] = Value::Null;
            row["diff_refs"] = Value::Null;
            row["lastMergeSourceCommit"] = Value::Null;
            row["lastMergeTargetCommit"] = Value::Null;
        }
        "redirect" => {
            return (
                StatusCode::FOUND,
                [(
                    header::LOCATION,
                    "https://untrusted.example.test/never-follow",
                )],
                "synthetic-token",
            )
                .into_response();
        }
        "denied" => return (StatusCode::FORBIDDEN, "synthetic-token").into_response(),
        "oversized" => return Json(json!({"payload":"x".repeat(4*1024*1024+1)})).into_response(),
        _ => {}
    }
    Json(row).into_response()
}
#[tokio::test]
async fn synthetic_pr_reads_validate_identity_rebuild_links_and_never_follow_remote_urls() {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        for fault in [
            "",
            "fork",
            "missing-commits",
            "secret",
            "identity",
            "commit",
            "redirect",
            "denied",
            "oversized",
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let prefix = match kind {
                ForgeKind::Github => "",
                ForgeKind::Gitlab => "/gitlab",
                ForgeKind::Azure => "/org",
            };
            let base = format!("http://{address}{prefix}");
            let app = Router::new()
                .fallback(remote)
                .with_state(Remote { kind, fault });
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = Forge::synthetic(
                ForgeConfiguration {
                    kind,
                    base: base.clone(),
                },
                "synthetic-token".into(),
                3,
            )
            .unwrap();
            let repository = match kind {
                ForgeKind::Github => "owner/repo",
                ForgeKind::Gitlab => "group/nested/repo",
                ForgeKind::Azure => "project/repo",
            };
            let result = client.pull_request(repository, 7).await;
            if ["", "fork", "missing-commits"].contains(&fault) {
                let pr = result.unwrap();
                assert_eq!(pr.number, 7);
                assert_eq!(pr.revision, 3);
                assert!(pr.draft);
                assert_eq!(pr.status, "active");
                assert_eq!(pr.source_branch, "feature/synthetic");
                assert_eq!(pr.target_branch, "main");
                assert_eq!(pr.fork, fault == "fork");
                assert_eq!(pr.source_commit.is_none(), fault == "missing-commits");
                assert!(pr.url.starts_with(&base));
                assert!(!pr.url.contains("untrusted"));
                assert!(
                    !serde_json::to_string(&pr)
                        .unwrap()
                        .contains("synthetic-token")
                );
            } else {
                let message = result.err().unwrap().to_string();
                assert!(!message.contains("synthetic-token"));
                assert!(!message.contains("untrusted"));
            }
            assert!(client.pull_request("../outside", 0).await.is_err());
            server.abort();
            let _ = server.await;
        }
    }
}
fn update(revision: u64, base: &str, token: Option<&str>) -> ForgeUpdate {
    ForgeUpdate {
        expected_revision: revision,
        configuration: ForgeConfiguration {
            kind: ForgeKind::Github,
            base: base.into(),
        },
        token: token.map(str::to_owned),
        clear_token: false,
    }
}
fn protect(value: &str, decrypt: bool) -> atelier_app::Result<String> {
    if decrypt {
        Ok(String::from_utf8(
            STANDARD.decode(
                value
                    .strip_prefix("synthetic-encrypted:")
                    .ok_or("Synthetic envelope required")?,
            )?,
        )?)
    } else {
        Ok(format!("synthetic-encrypted:{}", STANDARD.encode(value)))
    }
}
#[tokio::test]
async fn forge_rotations_scope_changes_vault_failure_and_restore_protect_stale_requests() {
    let temporary = tempfile::tempdir().unwrap();
    let backups = temporary.path().to_owned();
    let database = Database::open(&temporary.path().join("synthetic.sqlite")).unwrap();
    database
        .call(move |store| {
            let first = forge::configure(
                store,
                update(0, "https://github.example.test", Some("synthetic-token")),
                &mut protect,
            )?;
            assert_eq!(first.revision, 1);
            assert!(first.connections[0].token_set);
            assert!(!serde_json::to_string(&first)?.contains("synthetic-token"));
            let backup = BackupDirectory::open(&backups)?.create(store)?;
            assert!(
                forge::configure(
                    store,
                    update(0, "https://github.example.test", Some("synthetic-stale")),
                    &mut protect
                )
                .is_err()
            );
            assert!(
                forge::configure(
                    store,
                    update(1, "https://github.example.test", Some("synthetic-failure")),
                    &mut |value, decrypt| if decrypt {
                        protect(value, true)
                    } else {
                        Err("synthetic vault failure".into())
                    }
                )
                .is_err()
            );
            assert_eq!(forge::revision(store)?, 1);
            let rotated = forge::configure(
                store,
                update(1, "https://github.example.test", Some("synthetic-rotated")),
                &mut protect,
            )?;
            assert_eq!(rotated.revision, 2);
            assert!(
                forge::credentials(store, ForgeKind::Github, 1, &mut |_, _| panic!(
                    "Stale request queried vault"
                ))
                .is_err()
            );
            BackupDirectory::open(&backups)?.restore(store, &backup.name)?;
            assert_eq!(forge::revision(store)?, 3);
            assert!(!forge::public(store, &mut protect)?.connections[0].token_set);
            assert!(forge::credentials(store, ForgeKind::Github, 2, &mut protect).is_err());
            forge::configure(
                store,
                update(3, "https://github.example.test", Some("synthetic-new")),
                &mut protect,
            )?;
            let changed = forge::configure(
                store,
                update(4, "https://other.example.test", None),
                &mut protect,
            )?;
            assert_eq!(changed.revision, 5);
            assert!(!changed.connections[0].token_set);
            assert!(forge::credentials(store, ForgeKind::Github, 5, &mut protect).is_err());
            assert!(
                !serde_json::to_string(&store.settings::<Document>()?)?.contains("synthetic-new")
            );
            assert!(store.documents::<Document>(Documents::Cards)?.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
#[test]
fn forge_configuration_refuses_embedded_credentials_redirect_like_paths_and_wrong_azure_scope() {
    for (kind, base) in [
        (ForgeKind::Github, "http://github.example.test"),
        (
            ForgeKind::Github,
            "https://user:synthetic-token@github.example.test",
        ),
        (ForgeKind::Github, "https://github.example.test/path"),
        (
            ForgeKind::Gitlab,
            "https://gitlab.example.test/path%2Fother",
        ),
        (
            ForgeKind::Gitlab,
            "https://gitlab.example.test?token=synthetic-token",
        ),
        (ForgeKind::Azure, "https://dev.azure.com"),
        (ForgeKind::Azure, "https://other.example.test/org"),
        (ForgeKind::Azure, "https://dev.azure.com/org/project"),
    ] {
        assert!(
            forge::normalize(&ForgeConfiguration {
                kind,
                base: base.into()
            })
            .is_err()
        );
    }
    for (kind, base) in [
        (ForgeKind::Github, "https://github.com"),
        (ForgeKind::Gitlab, "https://gitlab.example.test/prefix/"),
        (ForgeKind::Azure, "https://dev.azure.com/synthetic-org"),
    ] {
        assert!(
            forge::normalize(&ForgeConfiguration {
                kind,
                base: base.into()
            })
            .is_ok()
        );
    }
}
