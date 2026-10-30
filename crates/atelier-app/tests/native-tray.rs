use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[cfg(target_os = "linux")]
static LINUX_DESKTOP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
#[ignore = "requires the final campaign and a disposable native desktop"]
async fn the_product_tray_removes_itself_on_owner_disconnect_without_opening_data() {
    #[cfg(target_os = "linux")]
    let _desktop = LINUX_DESKTOP.lock().await;
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
    let before = linux_items().await;
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
    let registration = linux_product_registration(&before, child.id().unwrap()).await;
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
        while linux_items().await.contains(&registration) {
            assert!(
                std::time::Instant::now() < deadline,
                "The desktop retained the product tray after owner disconnect"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
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
#[tokio::test]
#[ignore = "requires the final campaign and a disposable native StatusNotifier desktop"]
async fn the_product_dbus_menu_opens_its_session_and_quits_the_owned_server() {
    use atelier_app::infrastructure::{data_lock::DataLock, local_http::LocalHttp, session};
    use std::{collections::HashMap, time::Instant};
    use zbus::{OwnedObjectPath, OwnedVariant, Variant};

    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref(),
        Ok("1")
    );
    let _desktop = LINUX_DESKTOP.lock().await;
    let synthetic = tempfile::tempdir().unwrap();
    let data = synthetic.path().join("synthetic-data");
    let assets = synthetic.path().join("synthetic-ui");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(assets.join("index.html"), "Synthetic native tray journey").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let before = linux_items().await;
    let mut server = Command::new(env!("CARGO_BIN_EXE_atelier"))
        .arg("serve")
        .arg("--data-dir")
        .arg(&data)
        .arg("--assets")
        .arg(&assets)
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let address = format!("http://127.0.0.1:{port}");
    let http = LocalHttp::new().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(health) = http
            .authenticated_health(&address, || session::read_optional(&data))
            .await
            .unwrap()
        {
            assert_eq!(health["pid"], server.id().unwrap());
            break;
        }
        assert!(
            server.try_wait().unwrap().is_none(),
            "Synthetic server exited before tray qualification"
        );
        assert!(
            Instant::now() < deadline,
            "Synthetic server did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(DataLock::acquire(&data).is_err());
    let connection = zbus::connection::Builder::session()
        .method_timeout(Duration::from_secs(3))
        .build()
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let registration = loop {
        let mut candidates = vec![];
        for registration in linux_items()
            .await
            .into_iter()
            .filter(|item| !before.contains(item))
        {
            let separator = registration.find('/').unwrap();
            let item = zbus::Proxy::new(
                &connection,
                &registration[..separator],
                &registration[separator..],
                "org.kde.StatusNotifierItem",
            )
            .await
            .unwrap();
            if item
                .get_property::<String>("Id")
                .await
                .unwrap()
                .starts_with("atelier-rust-")
            {
                drop(item);
                candidates.push(registration);
            }
        }
        assert!(
            candidates.len() <= 1,
            "Ambiguous native product tray; no menu event sent"
        );
        if let Some(registration) = candidates.pop() {
            break registration;
        }
        assert!(
            Instant::now() < deadline,
            "Owned server tray did not register"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let separator = registration.find('/').unwrap();
    let destination = &registration[..separator];
    let item = zbus::Proxy::new(
        &connection,
        destination,
        &registration[separator..],
        "org.kde.StatusNotifierItem",
    )
    .await
    .unwrap();
    assert_eq!(std::env::var("XDG_CURRENT_DESKTOP").as_deref(), Ok("XFCE"));
    let helper = item.get_property::<String>("Id").await.unwrap();
    let helper: u32 = helper
        .strip_prefix("atelier-rust-")
        .unwrap()
        .parse()
        .unwrap();
    let environment = std::fs::read(format!("/proc/{helper}/environ")).unwrap();
    assert!(environment.len() <= 64 * 1024);
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ] {
        let expected = format!("{key}={}", std::env::var(key).unwrap());
        assert!(
            environment
                .split(|byte| *byte == 0)
                .any(|entry| entry == expected.as_bytes()),
            "The tray helper lost its desktop context: {key}"
        );
    }
    let path = item.get_property::<OwnedObjectPath>("Menu").await.unwrap();
    let menu = zbus::Proxy::new(
        &connection,
        destination,
        path.as_str(),
        "com.canonical.dbusmenu",
    )
    .await
    .unwrap();
    let _: bool = menu.call("AboutToShow", &(0i32,)).await.unwrap();
    // Exercise the public desktop protocol used by a native panel, never an internal callback.
    type Properties = Vec<(i32, HashMap<String, OwnedVariant>)>;
    let properties = async || -> Properties {
        menu.call(
            "GetGroupProperties",
            &(Vec::<i32>::new(), vec!["label", "enabled"]),
        )
        .await
        .unwrap()
    };
    let label = |properties: &HashMap<String, OwnedVariant>| {
        properties
            .get("label")
            .map(|value| {
                String::try_from(value.try_clone().unwrap())
                    .unwrap()
                    .replace('_', "")
            })
            .unwrap_or_default()
    };
    let find = async |expected: &str| {
        let matching: Vec<_> = properties()
            .await
            .into_iter()
            .filter(|(_, props)| label(props) == expected)
            .collect();
        assert_eq!(matching.len(), 1, "Native menu action missing or ambiguous");
        let (id, props) = matching.into_iter().next().unwrap();
        assert!(
            props
                .get("enabled")
                .map(|value| bool::try_from(value.try_clone().unwrap()).unwrap())
                .unwrap_or(true)
        );
        id
    };
    let open = find("Ouvrir Atelier / Open Atelier").await;
    assert!(!data.join("local-browser.html").exists());
    let _: () = menu
        .call("Event", &(open, "clicked", Variant::from(0i32), 0u32))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let labels: Vec<_> = properties()
            .await
            .iter()
            .map(|(_, props)| label(props))
            .collect();
        assert!(
            !labels.iter().any(|text| text.starts_with("Action refusée")),
            "Native Open action failed; owned browser session prepared: {}",
            data.join("local-browser.html").is_file()
        );
        if data.join("local-browser.html").is_file()
            && labels.iter().any(|text| text == "Prêt / Ready")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Native Open did not finish and prepare the owned browser session"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        http.authenticated_health(&address, || session::read_optional(&data))
            .await
            .unwrap()
            .is_some()
    );
    assert!(DataLock::acquire(&data).is_err());
    let quit = find("Quitter Atelier / Quit Atelier").await;
    let _: () = menu
        .call("Event", &(quit, "clicked", Variant::from(0i32), 0u32))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(20), server.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(
        http.authenticated_health(&address, || session::read_optional(&data))
            .await
            .unwrap()
            .is_none()
    );
    drop(DataLock::acquire(&data).expect("Native Quit left the data lock held"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while linux_items().await.contains(&registration) {
        assert!(
            Instant::now() < deadline,
            "Native Quit left the tray registered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(target_os = "linux")]
async fn linux_items() -> Vec<String> {
    let connection = zbus::connection::Builder::session()
        .method_timeout(Duration::from_secs(3))
        .build()
        .await
        .unwrap();
    let watcher = zbus::Proxy::new(
        &connection,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )
    .await
    .unwrap();
    assert!(
        watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .await
            .unwrap(),
        "The final journey requires a real isolated desktop host"
    );
    let items = watcher
        .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        .await
        .unwrap();
    assert!(items.len() <= 100 && items.iter().all(|item| item.len() <= 4096));
    items
}

#[cfg(target_os = "linux")]
async fn linux_product_registration(before: &[String], pid: u32) -> String {
    let expected = format!("atelier-rust-{pid}");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        for registration in linux_items()
            .await
            .into_iter()
            .filter(|item| !before.contains(item))
        {
            let separator = registration
                .find('/')
                .expect("Desktop item path is missing");
            let connection = zbus::connection::Builder::session()
                .method_timeout(Duration::from_secs(3))
                .build()
                .await
                .unwrap();
            let item = zbus::Proxy::new(
                &connection,
                &registration[..separator],
                &registration[separator..],
                "org.kde.StatusNotifierItem",
            )
            .await
            .unwrap();
            if item.get_property::<String>("Id").await.ok().as_deref() == Some(expected.as_str()) {
                assert_eq!(
                    item.get_property::<String>("Status").await.unwrap(),
                    "Active"
                );
                let pixels = item
                    .get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap")
                    .await
                    .unwrap();
                assert!(pixels.iter().any(|(width, height, bytes)| *width == 16
                    && *height == 16
                    && bytes.len() == 1024));
                drop(item);
                return registration;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "The product tray never registered with the native desktop"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
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
