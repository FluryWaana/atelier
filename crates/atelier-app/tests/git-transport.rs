use atelier_app::infrastructure::git_transport::HttpsAuthentication;
use atelier_core::contracts::{ForgeConfiguration, ForgeKind};

#[tokio::test]
async fn clone_connections_scope_credentials_to_github_gitlab_and_azure_paths() {
    use atelier_app::infrastructure::{database::Database, forge, project_clone};
    use atelier_core::{application::project_clone::CloneRequest, contracts::ForgeUpdate};
    for (kind, base, origin) in [
        (
            ForgeKind::Github,
            "https://github.example.test",
            "https://github.example.test/owner/repo.git",
        ),
        (
            ForgeKind::Gitlab,
            "https://gitlab.example.test/prefix",
            "https://gitlab.example.test/prefix/group/nested/repo.git",
        ),
        (
            ForgeKind::Azure,
            "https://dev.azure.com/org",
            "https://dev.azure.com/org/project/_git/repo",
        ),
    ] {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        database
            .call(move |store| {
                let mut protect = |value: &str, decrypt: bool| -> atelier_app::Result<String> {
                    if decrypt {
                        Ok(value
                            .strip_prefix("synthetic-envelope:")
                            .ok_or("synthetic envelope required")?
                            .into())
                    } else {
                        Ok(format!("synthetic-envelope:{value}"))
                    }
                };
                forge::configure(
                    store,
                    ForgeUpdate {
                        expected_revision: 0,
                        configuration: ForgeConfiguration {
                            kind,
                            base: base.into(),
                        },
                        token: Some("synthetic-clone-token".into()),
                        clear_token: false,
                    },
                    &mut protect,
                )?;
                let mut input = CloneRequest {
                    url: origin.into(),
                    parent: "/synthetic".into(),
                    directory: "repo".into(),
                    connection: Some(kind),
                    operation: "synthetic-clone".into(),
                    confirmed: true,
                };
                let auth = project_clone::authentication(store, &input, &mut protect)?.unwrap();
                assert_eq!(auth.origin(origin)?, origin);
                for invalid in [
                    origin.replace("https://", "https://other."),
                    format!("{origin}?token=synthetic-clone-token"),
                    "git@forge.example.test:owner/repo.git".into(),
                ] {
                    input.url = invalid;
                    assert!(project_clone::authentication(store, &input, &mut protect).is_err());
                }
                input.url = origin.into();
                input.connection = None;
                assert!(
                    project_clone::authentication(store, &input, &mut |_, _| panic!(
                        "Public cloning must not read credentials"
                    ))?
                    .is_none()
                );
                Ok(())
            })
            .await
            .unwrap();
    }
}

#[test]
fn authentication_is_scoped_to_the_exact_forge_site_and_repository() {
    for (kind, base, repository, origin) in [
        (
            ForgeKind::Github,
            "https://github.example.test",
            "owner/repo",
            "https://github.example.test/owner/repo.git",
        ),
        (
            ForgeKind::Gitlab,
            "https://gitlab.example.test:8443/prefix",
            "group/nested/repo",
            "https://gitlab.example.test:8443/prefix/group/nested/repo.git",
        ),
        (
            ForgeKind::Azure,
            "https://dev.azure.com/org",
            "project/repo",
            "https://org@dev.azure.com/org/project/_git/repo",
        ),
    ] {
        let authentication = HttpsAuthentication::new(
            ForgeConfiguration {
                kind,
                base: base.into(),
            },
            repository.into(),
            "synthetic-token".into(),
        )
        .unwrap();
        let canonical = authentication.origin(origin).unwrap();
        assert!(!canonical.contains('@'));
        for invalid in [
            origin.replace("https://", "http://"),
            origin.replace("repo", "other"),
            origin.replace("https://", "https://synthetic-token@"),
            format!("{origin}?token=synthetic-token"),
            format!("{origin}#fragment"),
            format!("{origin}/child"),
            format!(" {origin}"),
            origin.replacen("https://", "https:\n//", 1),
        ] {
            assert!(authentication.origin(&invalid).is_err(), "{invalid}");
        }
        assert!(
            authentication
                .origin(&origin.replace("repo", "repo-other"))
                .is_err()
        );
    }
    let authentication = HttpsAuthentication::new(
        ForgeConfiguration {
            kind: ForgeKind::Gitlab,
            base: "https://gitlab.example.test:8443/prefix".into(),
        },
        "group/repo".into(),
        "synthetic-token".into(),
    )
    .unwrap();
    for origin in [
        "https://gitlab.example.test/prefix/group/repo",
        "https://gitlab.example.test:8443/group/repo",
        "https://other.example.test:8443/prefix/group/repo",
        "git@gitlab.example.test:prefix/group/repo",
    ] {
        assert!(authentication.origin(origin).is_err());
    }
}

#[test]
fn implicit_transport_configuration_and_non_synthetic_loopback_credentials_are_refused() {
    for key in [
        "url.https://other/.insteadof",
        "include.path",
        "includeIf.gitdir.path",
        "http.extraHeader",
        "http.https://example.test/.proxy",
        "credential.helper",
        "core.askPass",
        "core.sshCommand",
        "extensions.worktreeConfig",
        "extensions.partialClone",
        "remote.origin.promisor",
        "remote.origin.uploadpack",
        "fetch.uriProtocols",
    ] {
        assert!(
            HttpsAuthentication::validate_configuration(&format!("core.bare\0{key}\0")).is_err(),
            "{key}"
        );
    }
    HttpsAuthentication::validate_configuration(
        "core.bare\0remote.origin.url\0remote.origin.fetch\0",
    )
    .unwrap();
    for base in [
        "http://localhost:8000",
        "http://192.0.2.1:8000",
        "http://127.0.0.1:8000?secret=x",
        "http://user@127.0.0.1:8000",
    ] {
        assert!(
            HttpsAuthentication::synthetic(
                ForgeConfiguration {
                    kind: ForgeKind::Github,
                    base: base.into()
                },
                "owner/repo".into(),
                "synthetic-token".into()
            )
            .is_err()
        );
    }
    assert!(
        HttpsAuthentication::synthetic(
            ForgeConfiguration {
                kind: ForgeKind::Github,
                base: "http://127.0.0.1:8000".into()
            },
            "owner/repo".into(),
            "not-a-synthetic-token".into()
        )
        .is_err()
    );
}
