#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

#[test]
fn safari_bootstrap_replaces_a_stale_browser_once_and_stops_on_configuration_failure() {
    for (running, fail_step, expected_status) in [
        (true, "none", 0),
        (false, "none", 0),
        (true, "stop", 24),
        (true, "enable", 23),
        (false, "launch", 25),
    ] {
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
        if running {
            fs::write(fixture.path().join("running"), "synthetic existing Safari").unwrap();
        }
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
            .env("FAIL_STEP", fail_step)
            .status()
            .unwrap();
        let commands = fs::read_to_string(fixture.path().join("commands")).unwrap();
        assert_eq!(
            status.code(),
            Some(expected_status),
            "{fail_step}: {commands}"
        );
        let launches = commands
            .lines()
            .filter(|line| line.starts_with("open "))
            .count();
        assert_eq!(
            launches,
            usize::from(matches!(fail_step, "none" | "launch"))
        );
        assert_eq!(
            fixture.path().join("running").exists(),
            matches!(fail_step, "none" | "stop")
        );
        if fail_step == "stop" {
            assert!(!commands.contains("safaridriver --enable"));
        } else if running {
            assert!(
                commands.find("pkill ").unwrap() < commands.find("safaridriver --enable").unwrap()
            );
        } else {
            assert!(!commands.contains("pkill "));
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
