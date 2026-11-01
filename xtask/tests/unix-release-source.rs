#![cfg(unix)]

#[test]
fn unix_release_requires_all_five_platforms_and_the_unchanged_published_tag() {
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/unix-release-source.sh"
            ))
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/packaging/verify-unix-release-source.sh"
            ))
            .arg(fixture.path())
            .status()
            .unwrap()
            .success()
    );
}
