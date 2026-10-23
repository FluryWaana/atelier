use atelier_app::infrastructure::token_vault::execute_keyring;
use std::time::{Duration, Instant};

#[tokio::test]
async fn keyring_stdin_eof_and_excessive_stderr_are_handled_without_hanging() {
    let fixture = env!("CARGO_BIN_EXE_native-fixture");
    let (status, value) = execute_keyring(fixture, &["keyring-echo"], Some("synthetic-é-🔑"))
        .await
        .unwrap();
    assert_eq!(status, 0);
    assert_eq!(value, "synthetic-é-🔑");
    let started = Instant::now();
    assert!(
        execute_keyring(fixture, &["keyring-flood"], None)
            .await
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn an_unresponsive_keyring_is_terminated_at_the_production_deadline() {
    let started = Instant::now();
    let error = execute_keyring(env!("CARGO_BIN_EXE_native-fixture"), &["idle"], None)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Le trousseau système ne répond pas.");
    assert!(started.elapsed() >= Duration::from_secs(15));
    assert!(started.elapsed() < Duration::from_secs(30));
}
