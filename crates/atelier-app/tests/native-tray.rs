use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[tokio::test]
#[ignore = "requires the final campaign and a disposable native desktop"]
async fn the_product_tray_removes_itself_on_owner_disconnect_without_opening_data() {
    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref(),
        Ok("1")
    );
    let synthetic = tempfile::tempdir().unwrap();
    let data = synthetic.path().join("never-opened-data");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    #[cfg(target_os = "linux")]
    let before = linux_items();
    let mut command = Command::new(env!("CARGO_BIN_EXE_atelier"));
    command
        .arg("tray-helper")
        .arg(&data)
        .arg(port.to_string())
        .arg(std::process::id().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut ready = [0; b"atelier-tray/1 ready\n".len()];
    tokio::time::timeout(Duration::from_secs(10), output.read_exact(&mut ready))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&ready, b"atelier-tray/1 ready\n");
    #[cfg(target_os = "linux")]
    let registration = linux_product_registration(&before, child.id().unwrap());
    assert!(!data.exists());
    drop(child.stdin.take());
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while linux_items().contains(&registration) {
            assert!(
                std::time::Instant::now() < deadline,
                "The desktop retained the product tray after owner disconnect"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(!data.exists());
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}

#[cfg(target_os = "linux")]
fn linux_items() -> Vec<String> {
    let connection = zbus::blocking::connection::Builder::session()
        .unwrap()
        .method_timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let watcher = zbus::blocking::Proxy::new(
        &connection,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )
    .unwrap();
    assert!(
        watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .unwrap(),
        "The final journey requires a real isolated desktop host"
    );
    let items = watcher
        .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        .unwrap();
    assert!(items.len() <= 100 && items.iter().all(|item| item.len() <= 4096));
    items
}

#[cfg(target_os = "linux")]
fn linux_product_registration(before: &[String], pid: u32) -> String {
    let expected = format!("atelier-rust-{pid}");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        for registration in linux_items()
            .into_iter()
            .filter(|item| !before.contains(item))
        {
            let separator = registration
                .find('/')
                .expect("Desktop item path is missing");
            let connection = zbus::blocking::connection::Builder::session()
                .unwrap()
                .method_timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            let item = zbus::blocking::Proxy::new(
                &connection,
                &registration[..separator],
                &registration[separator..],
                "org.kde.StatusNotifierItem",
            )
            .unwrap();
            if item.get_property::<String>("Id").ok().as_deref() == Some(expected.as_str()) {
                assert_eq!(item.get_property::<String>("Status").unwrap(), "Active");
                let pixels = item
                    .get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap")
                    .unwrap();
                assert!(pixels.iter().any(|(width, height, bytes)| *width == 16
                    && *height == 16
                    && bytes.len() == 1024));
                return registration;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "The product tray never registered with the native desktop"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test]
async fn the_product_tray_rejects_missing_or_foreign_arguments_before_native_ui() {
    let synthetic = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["relative", "4317"],
        vec!["--data-dir", "4317"],
        vec!["relative", "4317", "extra"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_atelier"))
            .arg("tray-helper")
            .args(args)
            .current_dir(synthetic.path())
            .output()
            .await
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}
