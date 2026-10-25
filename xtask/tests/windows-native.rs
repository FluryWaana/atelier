#![cfg(windows)]

#[path = "support/windows.rs"]
mod windows;

use std::{
    io::{BufRead, BufReader, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
};

struct Fixture {
    child: Child,
    output: mpsc::Receiver<String>,
}
impl Fixture {
    fn spawn(path: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(path)
            .args(args)
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, output }
    }
    fn line(&self) -> String {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("Native fixture did not respond")
    }
    fn exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "Native fixture did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn job() -> OwnedHandle {
    // A fresh unnamed job has one owned handle; closing it kills only processes assigned by this test.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // Windows copies the correctly sized structure during this call.
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    job
}

#[test]
fn job_close_stops_parent_and_descendant_without_touching_another_process() {
    let executable = Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    let mut unrelated = Fixture::spawn(executable, &["idle"]);
    assert_eq!(unrelated.line(), "ready");
    let mut parent = Fixture::spawn(executable, &["family"]);
    let job = job();
    assert_eq!(parent.line(), "ready");
    // The fixture cannot spawn its descendant until it receives the stdin gate after assignment.
    assert_ne!(
        unsafe { AssignProcessToJobObject(job.as_raw_handle(), parent.child.as_raw_handle()) },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    parent
        .child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"spawn\n")
        .unwrap();
    let descendant = windows::open_process(parent.line().parse().unwrap()).unwrap();
    assert_eq!(windows::wait(&descendant, 0), WAIT_TIMEOUT);
    drop(job);
    assert_eq!(windows::wait(&parent.child, 5000), WAIT_OBJECT_0);
    assert_eq!(windows::wait(&descendant, 5000), WAIT_OBJECT_0);
    assert!(unrelated.child.try_wait().unwrap().is_none());
}

#[tokio::test]
async fn suspended_launch_contains_immediate_detached_and_orphaned_children() {
    use atelier_app::infrastructure::process_tree::ContainedChild;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let executable = Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    let mut unrelated = Fixture::spawn(executable, &["idle"]);
    assert_eq!(unrelated.line(), "ready");
    for mode in ["normal", "detached", "orphan", "drop"] {
        let mut command = tokio::process::Command::new(executable);
        command
            .args([
                "family-immediate",
                if mode == "drop" { "normal" } else { mode },
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut parent = ContainedChild::spawn(&mut command).await.unwrap();
        let mut lines = BufReader::new(parent.child.stdout.take().unwrap()).lines();
        let pid = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let descendant = windows::open_process(pid.parse().unwrap()).unwrap();
        assert_eq!(windows::wait(&descendant, 0), WAIT_TIMEOUT);
        match mode {
            "orphan" => assert!(
                tokio::time::timeout(Duration::from_secs(10), parent.wait())
                    .await
                    .unwrap()
                    .unwrap()
                    .success()
            ),
            "drop" => drop(parent),
            _ => assert!(!parent.stop().await.unwrap().success()),
        }
        assert_eq!(windows::wait(&descendant, 5000), WAIT_OBJECT_0, "{mode}");
        assert!(unrelated.child.try_wait().unwrap().is_none());
    }
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["family-immediate", "breakaway"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    let mut parent = ContainedChild::spawn(&mut command).await.unwrap();
    let mut lines = BufReader::new(parent.child.stdout.take().unwrap()).lines();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .as_deref(),
        Some("breakaway-refused")
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(10), parent.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let fixture = tempfile::tempdir().unwrap();
    assert!(
        ContainedChild::spawn(&mut tokio::process::Command::new(
            fixture.path().join("missing.exe")
        ))
        .await
        .is_err()
    );
    assert!(unrelated.child.try_wait().unwrap().is_none());
}

#[tokio::test]
async fn captured_processes_drain_both_streams_and_clean_up_on_overflow_timeout_and_cancel() {
    use atelier_app::infrastructure::process_tree::capture;
    let executable = Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    let input = vec![b'i'; 200_000];
    let output = capture(
        tokio::process::Command::new(executable).arg("process-streams"),
        &input,
        Duration::from_secs(10),
        500_000,
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stderr, vec![b'e'; 200_000]);
    assert_eq!(output.stdout, [vec![b'o'; 200_000], input].concat());
    let mut unrelated = Fixture::spawn(executable, &["idle"]);
    assert_eq!(unrelated.line(), "ready");
    for mode in ["flood", "hang", "failure", "cancel", "guard"] {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(
            fixture.path().join(".process-fixture"),
            "atelier-m04-synthetic-only",
        )
        .unwrap();
        let directory = fixture.path().to_owned();
        let running = tokio::spawn(async move {
            let guarded_directory = directory.clone();
            atelier_app::infrastructure::process_tree::capture_guarded(
                tokio::process::Command::new(executable)
                    .args([
                        "process-output",
                        if ["cancel", "guard"].contains(&mode) {
                            "hang"
                        } else {
                            mode
                        },
                    ])
                    .arg(directory),
                &[],
                Duration::from_secs(5),
                1024,
                async move {
                    if mode != "guard" {
                        return std::future::pending::<atelier_app::Error>().await;
                    }
                    loop {
                        if guarded_directory.join("go").exists() {
                            return "Synthetic storage guard stopped download".into();
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                },
            )
            .await
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        let descendant = loop {
            if let Ok(value) = std::fs::read_to_string(fixture.path().join("pid"))
                && let Ok(pid) = value.parse()
            {
                break windows::open_process(pid).unwrap();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Synthetic descendant did not start: {mode}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(windows::wait(&descendant, 0), WAIT_TIMEOUT);
        if mode == "cancel" {
            running.abort();
            assert!(running.await.unwrap_err().is_cancelled());
        } else {
            std::fs::write(fixture.path().join("go"), "go").unwrap();
            let result = tokio::time::timeout(Duration::from_secs(12), running)
                .await
                .unwrap()
                .unwrap();
            match mode {
                "failure" => {
                    let output = result.unwrap();
                    assert_eq!(output.status.code(), Some(7));
                    assert_eq!(output.stderr, b"synthetic failure\n");
                }
                "flood" => assert_eq!(
                    result.unwrap_err().to_string(),
                    "Limite de sortie stderr dépassée."
                ),
                "guard" => assert_eq!(
                    result.unwrap_err().to_string(),
                    "Synthetic storage guard stopped download"
                ),
                _ => assert_eq!(
                    result.unwrap_err().to_string(),
                    "Durée maximale du processus dépassée."
                ),
            }
        }
        assert_eq!(windows::wait(&descendant, 5000), WAIT_OBJECT_0, "{mode}");
        assert!(unrelated.child.try_wait().unwrap().is_none());
    }
}

#[test]
fn staged_worker_verifies_identity_waits_and_replaces_a_real_executable() {
    use std::fs;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("stage")).unwrap();
    fs::write(root.join(".native-fixture"), "atelier-m01-synthetic-only").unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    for name in ["app.exe", "candidate.exe", "stage/worker.exe"] {
        fs::copy(executable, root.join(name)).unwrap();
    }
    fs::OpenOptions::new()
        .append(true)
        .open(root.join("candidate.exe"))
        .unwrap()
        .write_all(b"M01-CANDIDATE")
        .unwrap();
    let old_bytes = fs::read(root.join("app.exe")).unwrap();
    let mut app = Fixture::spawn(&root.join("app.exe"), &["idle"]);
    assert_eq!(app.line(), "ready");
    assert!(
        fs::copy(root.join("candidate.exe"), root.join("app.exe")).is_err(),
        "Windows must reject overwriting the running image"
    );
    let pid = app.child.id().to_string();
    let created = windows::creation_time(&app.child).unwrap();
    let root_arg = root.to_str().unwrap();
    let mut rejected = Fixture::spawn(
        &root.join("stage/worker.exe"),
        &["worker", root_arg, &pid, &(created + 1).to_string()],
    );
    assert!(!rejected.exit().success());
    assert_eq!(fs::read(root.join("app.exe")).unwrap(), old_bytes);
    let mut worker = Fixture::spawn(
        &root.join("stage/worker.exe"),
        &["worker", root_arg, &pid, &created.to_string()],
    );
    assert_eq!(worker.line(), "ready");
    assert!(worker.child.try_wait().unwrap().is_none());
    assert_eq!(fs::read(root.join("app.exe")).unwrap(), old_bytes);
    app.child.kill().unwrap();
    app.child.wait().unwrap();
    assert!(worker.exit().success());
    assert_eq!(
        fs::read(root.join("app.exe")).unwrap(),
        fs::read(root.join("candidate.exe")).unwrap()
    );
    let replaced = Fixture::spawn(&root.join("app.exe"), &["idle"]);
    assert_eq!(replaced.line(), "ready");
}
