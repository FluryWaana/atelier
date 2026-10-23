use std::{
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::RECT,
    UI::{
        Shell::{
            NIF_ICON, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICON_VERSION_4,
            NOTIFYICONDATAW, NOTIFYICONDATAW_0, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::{CreateWindowExW, DestroyWindow, IDI_APPLICATION, LoadIconW},
    },
};

struct Tray {
    data: NOTIFYICONDATAW,
    active: bool,
}
impl Drop for Tray {
    fn drop(&mut self) {
        // This probe owns the hidden window and notification ID on the current thread.
        unsafe {
            if self.active {
                Shell_NotifyIconW(NIM_DELETE, &self.data);
            }
            DestroyWindow(self.data.hWnd);
        }
    }
}

pub fn probe() -> Result<(), Box<dyn std::error::Error>> {
    // STATIC is a built-in class; all pointers are static strings or null, and the window stays hidden.
    let window = unsafe {
        CreateWindowExW(
            0,
            windows_sys::core::w!("STATIC"),
            windows_sys::core::w!("Atelier M01 — essai tray"),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        )
    };
    if window.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut tray = Tray {
        data: NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON | NIF_TIP,
            ..Default::default()
        },
        active: false,
    };
    // LoadIcon returns a shared system icon, which must not be destroyed by the caller.
    tray.data.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    if tray.data.hIcon.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    for (slot, unit) in tray
        .data
        .szTip
        .iter_mut()
        .zip("Atelier — essai Rust M01".encode_utf16())
    {
        *slot = unit;
    }
    // Shell copies this fully initialized structure; its window and shared icon remain alive.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &tray.data) } == 0 {
        return Err("Tray Windows indisponible : ouvrir une session de bureau.".into());
    }
    tray.active = true;
    tray.data.Anonymous = NOTIFYICONDATAW_0 {
        uVersion: NOTIFYICON_VERSION_4,
    };
    // The icon has just been registered with this exact identifier.
    if unsafe { Shell_NotifyIconW(NIM_SETVERSION, &tray.data) } == 0 {
        return Err("Version du tray refusée.".into());
    }
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: window,
        uID: 1,
        ..Default::default()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut rect = RECT::default();
    loop {
        // The output rectangle and identifier remain valid for this synchronous shell call.
        if unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } >= 0
            && rect.right > rect.left
            && rect.bottom > rect.top
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("L’icône ne possède pas de position dans le tray.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Deletion addresses only the notification owned by this probe.
    if unsafe { Shell_NotifyIconW(NIM_DELETE, &tray.data) } == 0 {
        return Err("Retrait de l’icône refusé.".into());
    }
    tray.active = false;
    // After deletion the same ID must no longer resolve to an icon.
    if unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } >= 0 {
        return Err("Icône encore présente après retrait.".into());
    }
    println!("Tray Windows : ajout, version 4, rectangle réel et retrait vérifiés.");
    Ok(())
}
