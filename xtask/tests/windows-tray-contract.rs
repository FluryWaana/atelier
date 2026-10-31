#![cfg(windows)]

#[test]
fn native_tray_error_evidence_is_scoped_bounded_and_keeps_the_original_failure() {
    let fixture = tempfile::tempdir().unwrap();
    let native = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    use std::os::windows::process::CommandExt;
    assert!(
        std::process::Command::new(native)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File"
            ])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/windows-tray-context.ps1"
            ))
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/qualify-tray-windows.ps1"
            ))
            .arg(fixture.path())
            .creation_flags(0x08000000)
            .status()
            .unwrap()
            .success()
    );
}
