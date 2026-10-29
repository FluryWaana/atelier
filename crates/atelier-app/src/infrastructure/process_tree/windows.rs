use crate::Result;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::ExitStatus,
    time::Duration,
};
use tokio::process::{Child, Command};
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

fn safe_script_argument(value: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().all(|unit| {
        unit >= 0x20
            && !matches!(
                unit,
                0x22 | 0x25 | 0x26 | 0x28 | 0x29 | 0x3c | 0x3e | 0x5e | 0x7c | 0x21
            )
    })
}

#[cfg(test)]
#[test]
fn batch_arguments_reject_shell_expansion_and_control_characters() {
    use std::ffi::OsStr;
    assert!(safe_script_argument(OsStr::new("model name-v2")));
    for value in ["a&b", "a|b", "%PATH%", "!name!", "a\nb", "a\"b", "(x)"] {
        assert!(!safe_script_argument(OsStr::new(value)));
    }
}

impl ContainedChild {
    pub async fn process_ids(&self) -> Result<Vec<u32>> {
        use windows_sys::Win32::{
            Foundation::ERROR_MORE_DATA,
            System::JobObjects::{JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList},
        };
        for capacity in [16usize, 64, 256, 1024, 4096] {
            let offset = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
            let size = offset + capacity * std::mem::size_of::<usize>();
            let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let mut returned = 0;
            // The usize buffer supplies the alignment and capacity required by the variable-length list.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.job.0.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    size as u32,
                    &mut returned,
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                    continue;
                }
                return Err(error.into());
            }
            // Windows initialized this header; validate its count before reading any array elements.
            let header = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let count = header.NumberOfProcessIdsInList as usize;
            if count > capacity || count < header.NumberOfAssignedProcesses as usize {
                continue;
            }
            let pointer = unsafe { buffer.as_ptr().cast::<u8>().add(offset).cast::<usize>() };
            let ids = unsafe { std::slice::from_raw_parts(pointer, count) };
            return ids
                .iter()
                .map(|id| u32::try_from(*id).map_err(Into::into))
                .collect();
        }
        Err("message.service_processes_too_many".into())
    }

    pub async fn spawn(command: &mut Command) -> Result<Self> {
        Self::spawn_ready(command, || Ok(())).await
    }
    pub(crate) async fn spawn_ready(
        command: &mut Command,
        ready: impl FnOnce() -> Result<()>,
    ) -> Result<Self> {
        let program = std::path::Path::new(command.as_std().get_program());
        if program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            })
            && (!safe_script_argument(program.as_os_str())
                || !command.as_std().get_args().all(safe_script_argument))
        {
            return Err(
                "Ce lanceur Windows ne peut pas recevoir ces caractères dans ses arguments.".into(),
            );
        }
        let job = Job::new()?;
        command
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let attach = (|| -> Result<()> {
            let pid = child.id().ok_or("message.startup_process_missing")?;
            job.attach(child.raw_handle().ok_or("Handle de démarrage absent.")?)?;
            let thread = primary_thread(pid)?;
            ready()?;
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
            .map_err(|_| "message.process_stop_unconfirmed")?
    }
}
