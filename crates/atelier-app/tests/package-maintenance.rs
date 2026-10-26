use std::process::Command;

#[test]
fn recovery_requires_an_explicit_absolute_scope_and_confirmation_before_access() {
    let synthetic = tempfile::tempdir().unwrap();
    let absent = synthetic.path().join("must-not-be-created");
    for args in [
        vec![],
        vec!["--root", "relative", "--confirm"],
        vec!["--root", "relative"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_atelier"))
            .arg("recover-package-maintenance")
            .args(args)
            .current_dir(synthetic.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    let result = Command::new(env!("CARGO_BIN_EXE_atelier"))
        .args(["recover-package-maintenance", "--root"])
        .arg(&absent)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!absent.exists());
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn recovery_never_lifts_a_marker_on_an_unsigned_or_partly_removed_installation() {
    let synthetic = tempfile::tempdir().unwrap();
    std::fs::write(synthetic.path().join(".package-lock"), []).unwrap();
    let marker = synthetic.path().join(".package-maintenance");
    std::fs::write(&marker, b"atelier-package-maintenance/1\n").unwrap();
    std::fs::write(synthetic.path().join("human-file"), "preserve").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_atelier"))
        .args(["recover-package-maintenance", "--root"])
        .arg(synthetic.path())
        .arg("--confirm")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read(marker).unwrap(),
        b"atelier-package-maintenance/1\n"
    );
    assert_eq!(
        std::fs::read_to_string(synthetic.path().join("human-file")).unwrap(),
        "preserve"
    );
    assert_eq!(std::fs::read_dir(synthetic.path()).unwrap().count(), 3);
}
