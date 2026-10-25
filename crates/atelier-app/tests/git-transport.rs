use atelier_app::infrastructure::git_transport::HttpsAuthentication;
use atelier_core::contracts::{ForgeConfiguration, ForgeKind};

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
