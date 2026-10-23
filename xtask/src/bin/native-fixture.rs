use std::{
    io::{self, Write},
    process::{Command, Stdio},
    time::Duration,
};

#[cfg(windows)]
#[path = "../../tests/support/windows.rs"]
mod windows;

#[cfg(windows)]
#[path = "../../tests/support/tray.rs"]
mod tray;

fn ready(value: impl std::fmt::Display) -> io::Result<()> {
    println!("{value}");
    io::stdout().flush()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["idle"] => {
            ready("ready")?;
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        ["family"] => {
            ready("ready")?;
            let mut gate = String::new();
            io::stdin().read_line(&mut gate)?;
            if gate.trim() != "spawn" {
                return Err("Missing test gate".into());
            }
            let mut command = Command::new(std::env::current_exe()?);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .arg("idle")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            ready(child.id())?;
            child.wait()?;
            Ok(())
        }
        #[cfg(windows)]
        ["tray"] => tray::probe(),
        #[cfg(windows)]
        ["worker", directory, pid, created] => {
            use std::fs;
            let directory = fs::canonicalize(directory)?;
            if fs::read_to_string(directory.join(".native-fixture"))?
                != "atelier-m01-synthetic-only"
            {
                return Err("Not a native test fixture".into());
            }
            if fs::canonicalize(std::env::current_exe()?)?
                != fs::canonicalize(directory.join("stage/worker.exe"))?
            {
                return Err("Worker must run from its staged copy".into());
            }
            for name in ["app.exe", "candidate.exe"] {
                if !fs::symlink_metadata(directory.join(name))?.is_file() {
                    return Err("Regular fixture executable required".into());
                }
            }
            let process = windows::open_process(pid.parse()?)?;
            if windows::creation_time(&process)? != created.parse::<u64>()? {
                return Err("Process identity mismatch".into());
            }
            ready("ready")?;
            if windows::wait(&process, 10_000) != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
                return Err("Process did not exit; replacement refused".into());
            }
            // shortcut: this is a synthetic feasibility probe, not the transactional M09 updater.
            fs::copy(directory.join("candidate.exe"), directory.join("app.exe"))?;
            Ok(())
        }
        _ => Err(
            "Native test fixture only: idle | family | worker DIRECTORY PID CREATION_TIME | tray"
                .into(),
        ),
    }
}
