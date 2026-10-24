use crate::Result;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::ExitStatus,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, GetProcessIdOfThread, OpenThread, ResumeThread,
            THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

struct Job(OwnedHandle);

impl Job {
    fn new() -> Result<Self> {
        // The unnamed, noninheritable job is owned only by this guard.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // Windows copies this correctly sized structure and no breakaway flag is permitted.
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        Ok(job)
    }

    fn attach(&self, process: HANDLE) -> Result<()> {
        // The caller retains the suspended child's process handle through this call.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn terminate(&self) -> Result<()> {
        // This owned job contains only the root created here and its descendants.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }

    async fn wait_empty(&self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                // The job remains owned while Windows writes the accounting structure.
                if unsafe {
                    QueryInformationJobObject(
                        self.0.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        std::mem::size_of_val(&accounting) as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                if accounting.ActiveProcesses == 0 {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| "L’arrêt de l’arbre de processus n’est pas confirmé.")?
    }
}

fn primary_thread(pid: u32) -> Result<OwnedHandle> {
    // CREATE_SUSPENDED keeps the sole primary thread from executing application code.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // The snapshot and structure remain live throughout this bounded enumeration.
    if unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut selected = None;
    loop {
        if entry.th32OwnerProcessID == pid && selected.replace(entry.th32ThreadID).is_some() {
            return Err("Plusieurs threads avant démarrage : lancement refusé.".into());
        }
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error.into());
            }
            break;
        }
    }
    let tid = selected.ok_or("Thread de démarrage introuvable.")?;
    // The suspended child is still held by its process handle, preventing PID reuse here.
    let raw = unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            tid,
        )
    };
    if raw.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
        return Err("Le thread ne correspond plus au processus lancé.".into());
    }
    Ok(thread)
}

pub struct ContainedChild {
    pub child: Child,
    job: Job,
}

impl ContainedChild {
    pub async fn spawn(command: &mut Command) -> Result<Self> {
        let job = Job::new()?;
        command
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let attach = (|| -> Result<()> {
            let pid = child.id().ok_or("Processus de démarrage absent.")?;
            job.attach(child.raw_handle().ok_or("Handle de démarrage absent.")?)?;
            let thread = primary_thread(pid)?;
            // Resume only after assignment; all children inherit the non-breakaway job.
            if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
                return Err("Suspension initiale du processus non confirmée.".into());
            }
            Ok(())
        })();
        if let Err(error) = attach {
            let _ = child.kill().await;
            return Err(error);
        }
        Ok(Self { child, job })
    }

    pub async fn wait(&mut self) -> Result<ExitStatus> {
        let status = self.child.wait().await?;
        self.job.terminate()?;
        self.job.wait_empty().await?;
        Ok(status)
    }

    pub async fn stop(&mut self) -> Result<ExitStatus> {
        self.job.terminate()?;
        tokio::time::timeout(Duration::from_secs(10), self.wait())
            .await
            .map_err(|_| "L’arrêt du processus n’est pas confirmé.")?
    }
}

async fn bounded(reader: impl AsyncRead + Unpin, limit: usize, stream: &str) -> Result<Vec<u8>> {
    let count = u64::try_from(limit.checked_add(1).ok_or("Limite de sortie invalide.")?)?;
    let mut bytes = Vec::new();
    reader.take(count).read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        return Err(format!("Limite de sortie {stream} dépassée.").into());
    }
    Ok(bytes)
}

pub async fn capture(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
    limit: usize,
) -> Result<std::process::Output> {
    use std::process::Stdio;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ContainedChild::spawn(command).await?;
    let mut stdin = child
        .child
        .stdin
        .take()
        .ok_or("Entrée du processus absente.")?;
    let stdout = child
        .child
        .stdout
        .take()
        .ok_or("Sortie du processus absente.")?;
    let stderr = child
        .child
        .stderr
        .take()
        .ok_or("Sortie d’erreur du processus absente.")?;
    let operation = async {
        let input = async {
            // Like Node runCommand, a child may close stdin without consuming the whole input.
            let _ = stdin.write_all(input).await;
            drop(stdin);
            Ok::<(), crate::Error>(())
        };
        let (stdout, stderr, (), status) = tokio::try_join!(
            bounded(stdout, limit, "stdout"),
            bounded(stderr, limit, "stderr"),
            input,
            child.wait()
        )?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    let result = match tokio::time::timeout(timeout, operation).await {
        Ok(result) => result,
        Err(_) => Err("Durée maximale du processus dépassée.".into()),
    };
    if result.is_err() {
        child.stop().await?;
    }
    result
}
