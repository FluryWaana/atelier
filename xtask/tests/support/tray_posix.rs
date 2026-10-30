use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn icon() -> Result<TrayIcon> {
    Ok(TrayIconBuilder::new()
        .with_id(format!("atelier-native-test-{}", std::process::id()))
        .with_tooltip("Atelier — essai synthétique")
        .with_icon(Icon::from_rgba(
            [40, 110, 220, 255].repeat(16 * 16),
            16,
            16,
        )?)
        .build()?)
}

#[cfg(target_os = "linux")]
pub fn probe() -> Result<()> {
    zbus::block_on(async {
        use std::time::{Duration, Instant};
        use zbus::{Proxy, connection::Builder};

        let connection = Builder::session()
            .method_timeout(Duration::from_secs(3))
            .build()
            .await?;
        let watcher = Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )
        .await?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .await
            .unwrap_or(false)
        {
            if Instant::now() >= deadline {
                return Err("A native StatusNotifier desktop host is required".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let before: Vec<String> = watcher
            .get_property("RegisteredStatusNotifierItems")
            .await?;
        let tray = icon()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let registration = loop {
            let items: Vec<String> = watcher
                .get_property("RegisteredStatusNotifierItems")
                .await?;
            if let Some(item) = items.into_iter().find(|item| !before.contains(item)) {
                break item;
            }
            if Instant::now() >= deadline {
                return Err("Tray did not register with the native desktop".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let separator = registration.find('/').ok_or("Missing tray object path")?;
        let item = Proxy::new(
            &connection,
            &registration[..separator],
            &registration[separator..],
            "org.kde.StatusNotifierItem",
        )
        .await?;
        assert_eq!(item.get_property::<String>("Id").await?, tray.id().as_ref());
        assert_eq!(item.get_property::<String>("Status").await?, "Active");
        let pixels: Vec<(i32, i32, Vec<u8>)> = item.get_property("IconPixmap").await?;
        assert!(
            pixels
                .iter()
                .any(|(w, h, bytes)| *w == 16 && *h == 16 && bytes.len() == 1024)
        );
        drop(tray);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let items: Vec<String> = watcher
                .get_property("RegisteredStatusNotifierItems")
                .await?;
            if !items.contains(&registration) {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Native desktop retained the removed tray".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("Native Linux tray registered, pixels verified and registration removed");
        Ok(())
    })
}

#[cfg(target_os = "macos")]
pub fn probe() -> Result<()> {
    use block2::RcBlock;
    use objc2::{MainThreadMarker, rc::autoreleasepool};
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSDate, NSRunLoop, NSTimer};
    use std::{cell::RefCell, ptr::NonNull};

    thread_local! {
        static CREATED: RefCell<Option<Result<TrayIcon>>> = const { RefCell::new(None) };
    }
    autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().ok_or("Tray requires the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let callback = RcBlock::new(|_: NonNull<NSTimer>| {
            CREATED.with(|created| *created.borrow_mut() = Some(icon()));
        });
        // The captureless block is Send; the main run loop owns all AppKit objects.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.01, false, &callback)
        };
        let pump = || {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.5));
        };
        pump();
        timer.invalidate();
        let tray = CREATED
            .with(|created| created.borrow_mut().take())
            .ok_or("Main run loop did not create the tray")??;
        let status = tray.ns_status_item().ok_or("Missing native status item")?;
        let button = status.button(mtm).ok_or("Missing native status button")?;
        let window = button.window().ok_or("Missing native status window")?;
        assert!(window.isVisible(), "Status window is not visible");
        assert!(button.frame().size.width > 0.0 && button.frame().size.height > 0.0);
        drop(tray);
        pump();
        assert!(button.window().is_none_or(|window| !window.isVisible()));
        println!("Native macOS tray window visible and removed on the main run loop");
        Ok(())
    })
}
