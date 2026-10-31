#![cfg(windows)]

#[test]
fn release_requires_the_exact_successful_main_campaign_and_all_windows_jobs() {
    use std::os::windows::process::CommandExt;
    let native = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
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
                "/tests/fixtures/release-source.ps1"
            ))
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/verify-release-source.ps1"
            ))
            .creation_flags(0x08000000)
            .status()
            .unwrap()
            .success()
    );
}
