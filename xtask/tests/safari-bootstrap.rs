#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

#[test]
fn safari_bootstrap_replaces_a_stale_browser_once_and_stops_on_configuration_failure() {
    for fail_enable in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let binary = fixture.path().join("bin");
        fs::create_dir(&binary).unwrap();
        let mock = fixture.path().join("command");
        fs::write(
            &mock,
            include_bytes!("fixtures/safari-bootstrap-command.sh"),
        )
        .unwrap();
        fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
        for name in ["uname", "pgrep", "pkill", "sudo", "safaridriver", "open"] {
            symlink(&mock, binary.join(name)).unwrap();
        }
        fs::write(fixture.path().join("running"), "synthetic existing Safari").unwrap();
        let path = std::env::join_paths([binary, "/usr/bin".into(), "/bin".into()]).unwrap();
        let status = Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/prepare-safari.sh"
            ))
            .env_clear()
            .env("PATH", path)
            .env("CI", "true")
            .env("ATELIER_RUST_FINAL_CAMPAIGN", "1")
            .env("SAFARI_FIXTURE", fixture.path())
            .env("FAIL_ENABLE", if fail_enable { "1" } else { "0" })
            .status()
            .unwrap();
        let commands = fs::read_to_string(fixture.path().join("commands")).unwrap();
        assert!(commands.find("pkill ").unwrap() < commands.find("safaridriver --enable").unwrap());
        if fail_enable {
            assert_eq!(status.code(), Some(23));
            assert!(!commands.contains("open "));
            assert!(!fixture.path().join("running").exists());
        } else {
            assert!(status.success());
            assert_eq!(
                commands
                    .lines()
                    .filter(|line| line.starts_with("open "))
                    .count(),
                1
            );
            assert!(fixture.path().join("running").exists());
        }
    }
}

#[test]
fn safari_bootstrap_refuses_a_workstation_without_ci_authorization() {
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packaging/prepare-safari.sh"
        ))
        .env_remove("CI")
        .env_remove("ATELIER_RUST_FINAL_CAMPAIGN")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
