#![cfg(windows)]
use atelier_app::infrastructure::{database::Database, store::Documents};
use serde_json::json;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetClassNameW, GetWindowThreadProcessId, IDNO, IDOK, IDYES,
        IsWindowVisible, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COMMAND, WM_GETTEXT, WM_SETTEXT,
    },
};

fn owned_dialog(parent: u32) -> Option<HWND> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    // The snapshot is read only to select direct helpers owned by this synthetic CLI process.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let _snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut pids = vec![];
    let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
    while next != 0 {
        if entry.th32ParentProcessID == parent {
            pids.push(entry.th32ProcessID);
        }
        next = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    struct Search {
        pids: Vec<u32>,
        result: Option<HWND>,
    }
    unsafe extern "system" fn visit(window: HWND, context: isize) -> i32 {
        // EnumWindows invokes this synchronously with the live Search value below.
        let search = unsafe { &mut *(context as *mut Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
        }
        if search.pids.contains(&pid) {
            let mut class = [0u16; 64];
            let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
            if length > 0
                && String::from_utf16_lossy(&class[..length as usize]) == "#32770"
                && unsafe { IsWindowVisible(window) } != 0
            {
                search.result = Some(window);
                return 0;
            }
        }
        1
    }
    let mut search = Search { pids, result: None };
    unsafe {
        EnumWindows(Some(visit), (&mut search as *mut Search) as isize);
    }
    search.result
}

fn edit_control(dialog: HWND) -> Option<HWND> {
    unsafe extern "system" fn visit(window: HWND, context: isize) -> i32 {
        let result = unsafe { &mut *(context as *mut Option<HWND>) };
        let mut class = [0u16; 64];
        let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
        if length > 0
            && String::from_utf16_lossy(&class[..length as usize]) == "Edit"
            && unsafe { IsWindowVisible(window) } != 0
        {
            *result = Some(window);
            return 0;
        }
        1
    }
    let mut result = None;
    // Only controls belonging to the already verified synthetic helper dialog are enumerated.
    unsafe {
        EnumChildWindows(
            dialog,
            Some(visit),
            (&mut result as *mut Option<HWND>) as isize,
        );
    }
    result
}

fn reviewed_paths(dialog: HWND) -> String {
    unsafe extern "system" fn visit(window: HWND, context: isize) -> i32 {
        // Only static text belonging to the already verified helper dialog is inspected.
        let result = unsafe { &mut *(context as *mut String) };
        let mut class = [0u16; 64];
        let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
        if length > 0 && String::from_utf16_lossy(&class[..length as usize]) == "Static" {
            let mut text = [0u16; 8192];
            let mut written = 0;
            let received = unsafe {
                SendMessageTimeoutW(
                    window,
                    WM_GETTEXT,
                    text.len(),
                    text.as_mut_ptr() as isize,
                    SMTO_ABORTIFHUNG,
                    3000,
                    &mut written,
                )
            };
            if received != 0 && written < text.len() {
                let text = String::from_utf16_lossy(&text[..written]);
                if text.contains("Source :") {
                    *result = text;
                    return 0;
                }
            }
        }
        1
    }
    let mut result = String::new();
    unsafe {
        EnumChildWindows(dialog, Some(visit), (&mut result as *mut String) as isize);
    }
    result
}

fn send(window: HWND, message: u32, value: usize, data: isize) {
    let mut result = 0;
    // WM_SETTEXT/WM_COMMAND are system messages; Windows marshals the text synchronously across processes.
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                window,
                message,
                value,
                data,
                SMTO_ABORTIFHUNG,
                3000,
                &mut result,
            )
        },
        0
    );
}

async fn dialog(parent: u32, previous: Option<HWND>) -> HWND {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(window) = owned_dialog(parent).filter(|window| Some(*window) != previous) {
            return window;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "synthetic native dialog not found"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
#[ignore = "requires the final campaign and a disposable Windows desktop"]
async fn the_native_transfer_selects_reviews_cancels_then_copies_without_mutating_its_source() {
    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref(),
        Ok("1")
    );
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("synthetic-source");
    std::fs::create_dir(&source).unwrap();
    let database = Database::open(&source.join("kanban.sqlite")).unwrap();
    database.call(|store|store.put(Documents::Cards, &json!({"id":"native-import-card","title":"Synthetic source preserved","status":"todo","projectId":"default"}))).await.unwrap();
    database.close().await.unwrap();
    let original = std::fs::read(source.join("kanban.sqlite")).unwrap();
    std::fs::write(
        source.join("human-note.txt"),
        "preserve this synthetic note",
    )
    .unwrap();
    for accept in [false, true] {
        let destination = root
            .path()
            .join(if accept { "accepted" } else { "cancelled" });
        let child = Command::new(env!("CARGO_BIN_EXE_atelier"))
            .args(["import-data", "--choose-source", "--destination"])
            .arg(&destination)
            .arg("--confirm")
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let parent = child.id().unwrap();
        let choosing = dialog(parent, None).await;
        let edit =
            edit_control(choosing).expect("folder dialog must expose its accessible path edit box");
        let path: Vec<u16> = source
            .to_str()
            .unwrap()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        send(edit, WM_SETTEXT, 0, path.as_ptr() as isize);
        send(choosing, WM_COMMAND, IDOK as usize, 0);
        let confirming = dialog(parent, Some(choosing)).await;
        let review = reviewed_paths(confirming).replace("\r\n", "\n");
        assert!(
            review.contains(&format!("Source : {}\n", source.display())),
            "source mismatch: no copy may be confirmed"
        );
        assert!(
            review.contains(&format!("Destination : {}\n", destination.display())),
            "destination mismatch: no copy may be confirmed"
        );
        send(
            confirming,
            WM_COMMAND,
            if accept { IDYES } else { IDNO } as usize,
            0,
        );
        let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if accept {
            let imported = Database::open(&destination.join("kanban.sqlite")).unwrap();
            imported
                .call(|store| {
                    let card: Option<serde_json::Value> =
                        store.document(Documents::Cards, "native-import-card")?;
                    assert_eq!(card.unwrap()["title"], "Synthetic source preserved");
                    Ok(())
                })
                .await
                .unwrap();
            imported.close().await.unwrap();
            assert_eq!(
                std::fs::read_to_string(destination.join("human-note.txt")).unwrap(),
                "preserve this synthetic note"
            );
        } else {
            assert!(!destination.exists());
        }
        assert_eq!(
            std::fs::read(source.join("kanban.sqlite")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(source.join("human-note.txt")).unwrap(),
            "preserve this synthetic note"
        );
    }
}
