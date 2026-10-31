#![cfg(unix)]

use std::{fs, process::Command};

#[test]
fn desktop_selection_targets_only_windows_setup_and_safari_and_preserves_other_modes() {
    let workflow = include_str!("../../.github/workflows/rust.yml");
    let script = workflow
        .lines()
        .skip_while(|line| !line.starts_with("          case \"$REQUESTED_SCOPE\""))
        .take_while(|line| !line.starts_with("  checks:"))
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(script.starts_with("case "));
    let all = [
        "windows-latest",
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "macos-15-intel",
        "macos-15",
    ];
    for (scope, platform, browser, all_platforms, targets, browsers) in [
        (
            "desktop",
            "windows",
            "chrome",
            "false",
            vec![all[0]],
            vec![all[4]],
        ),
        ("desktop", "all", "all", "true", vec![all[0]], vec![all[4]]),
        (
            "full",
            "all",
            "all",
            "false",
            all.to_vec(),
            vec![all[0], all[1], all[4]],
        ),
        (
            "full",
            "linux",
            "safari",
            "false",
            vec![all[1]],
            vec![all[1]],
        ),
        (
            "full",
            "macos-intel",
            "chrome",
            "false",
            vec![all[3]],
            vec![all[4]],
        ),
        (
            "native",
            "windows",
            "edge",
            "true",
            all.to_vec(),
            vec![all[0]],
        ),
        (
            "browser",
            "windows",
            "safari",
            "false",
            vec![all[0]],
            vec![all[4]],
        ),
        (
            "delivery",
            "linux-arm",
            "chrome",
            "false",
            vec![all[2]],
            vec![all[1]],
        ),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let output = fixture.path().join("output");
        assert!(
            Command::new("bash")
                .args(["-euo", "pipefail", "-c", &script])
                .env_clear()
                .env("REQUESTED_SCOPE", scope)
                .env("REQUESTED_PLATFORM", platform)
                .env("REQUESTED_BROWSER", browser)
                .env("REQUESTED_JOURNEY", "all")
                .env("RUN_ALL_PLATFORMS", all_platforms)
                .env("GITHUB_OUTPUT", &output)
                .env("GITHUB_STEP_SUMMARY", fixture.path().join("summary"))
                .status()
                .unwrap()
                .success()
        );
        let output = fs::read_to_string(output).unwrap();
        for (key, expected) in [("target_os", targets), ("browser_os", browsers)] {
            let value = output
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key}=")))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Vec<String>>(value).unwrap(),
                expected,
                "{scope}/{platform}/{browser}/{all_platforms}"
            );
        }
    }
    for (scope, platform) in [("unknown", "windows"), ("desktop", "unknown")] {
        assert!(
            !Command::new("bash")
                .args(["-euo", "pipefail", "-c", &script])
                .env_clear()
                .env("REQUESTED_SCOPE", scope)
                .env("REQUESTED_PLATFORM", platform)
                .env("REQUESTED_BROWSER", "all")
                .env("REQUESTED_JOURNEY", "all")
                .env("RUN_ALL_PLATFORMS", "false")
                .status()
                .unwrap()
                .success()
        );
    }
}
