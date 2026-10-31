use std::{fs, process::Command};

#[test]
fn prepared_ui_reaches_the_driver_in_a_moved_workspace_without_cargo() {
    for prepared in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let assets = fixture.path().join(".artifacts/rust-ui");
        let binaries = fixture.path().join("target/debug");
        fs::create_dir_all(&assets).unwrap();
        fs::create_dir_all(&binaries).unwrap();
        for name in [
            "index.html",
            "atelier-loader.js",
            "atelier-ui-synthetic.js",
            "atelier-ui-synthetic.wasm",
            "style-synthetic.css",
        ] {
            fs::write(assets.join(name), b"synthetic asset").unwrap();
        }
        for name in ["atelier", "native-fixture"] {
            fs::write(
                binaries.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
                b"synthetic binary, never executed",
            )
            .unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.args(["test-ui", "--prepared-assets"]);
        if prepared {
            command.arg("--prepared-binaries");
        }
        // The test harness rejects the driver's port argument before any browser starts.
        let result = command
            .env("GITHUB_WORKSPACE", fixture.path())
            .env("PATH", "")
            .env("ATELIER_TEST_BROWSER", "edge")
            .env("ATELIER_TEST_JOURNEY", "application")
            .env("ATELIER_WEBDRIVER", std::env::current_exe().unwrap())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("ATELIER_TEST_SERVER")
            .env_remove("ATELIER_TEST_ASSETS")
            .output()
            .unwrap();
        assert!(!result.status.success());
        let logs: Vec<_> = fs::read_dir(fixture.path().join(".artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "log"))
            .collect();
        assert_eq!(logs.len(), usize::from(prepared), "{result:?}");
        if prepared {
            assert!(fs::read_to_string(&logs[0]).unwrap().contains("port"));
        }
    }
}
