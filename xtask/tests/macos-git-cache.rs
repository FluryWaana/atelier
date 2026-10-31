#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

#[test]
fn cached_macos_git_is_reused_only_with_the_expected_version_and_tls_backend() {
    for (git, curl, valid_links, accepted) in [
        ("git version 2.56.0", "curl OpenSSL", true, true),
        ("git version 2.55.0", "curl OpenSSL", true, false),
        ("git version 2.56.0", "curl SecureTransport", true, false),
        ("git version 2.56.0", "curl OpenSSL", false, false),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let binary = fixture.path().join("bin");
        let curl_bin = fixture.path().join("curl/bin");
        let git_bin = fixture.path().join("atelier-git-openssl/bin");
        for directory in [&binary, &curl_bin, &git_bin] {
            fs::create_dir_all(directory).unwrap();
        }
        let mock = fixture.path().join("command");
        fs::write(&mock, include_bytes!("fixtures/macos-git-cache-command.sh")).unwrap();
        fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
        for name in ["uname", "brew", "otool", "make"] {
            symlink(&mock, binary.join(name)).unwrap();
        }
        symlink(&mock, curl_bin.join("curl")).unwrap();
        symlink(&mock, git_bin.join("git")).unwrap();
        let path = std::env::join_paths([binary, "/usr/bin".into(), "/bin".into()]).unwrap();
        let github_path = fixture.path().join("github-path");
        let links = if valid_links {
            format!("{}/curl/lib/libcurl.4.dylib", fixture.path().display())
        } else {
            "/usr/lib/libcurl.4.dylib".into()
        };
        let status = Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/install-macos-git.sh"
            ))
            .env_clear()
            .env("PATH", path)
            .env("CI", "true")
            .env("ATELIER_RUST_FINAL_CAMPAIGN", "1")
            .env("RUNNER_TEMP", fixture.path())
            .env("GITHUB_PATH", &github_path)
            .env("GIT_CACHE_FIXTURE", fixture.path())
            .env("GIT_VERSION", git)
            .env("CURL_VERSION", curl)
            .env("GIT_LINKS", links)
            .status()
            .unwrap();
        assert_eq!(status.success(), accepted, "{git}, {curl}, {valid_links}");
        assert_eq!(github_path.exists(), accepted);
        if accepted {
            assert_eq!(
                fs::read_to_string(github_path).unwrap().trim(),
                git_bin.to_str().unwrap()
            );
        }
    }
}
