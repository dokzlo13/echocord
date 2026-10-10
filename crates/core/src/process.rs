//! Windows session-process ownership. No protocol or audio is handled here.
//!
//! Requires Windows 10 or later (creation-time `PROC_THREAD_ATTRIBUTE_JOB_LIST`).
//! A failed job assignment fails the launch; there is no unowned fallback.
#![cfg(windows)]

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    marker::PhantomData,
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::SECURITY_ATTRIBUTES,
    System::{
        JobObjects::{
            CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Pipes::CreatePipe,
        Threading::{
            CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList,
            EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

/// Owns one engine and its non-inherited, unnamed kill-on-close job.
///
/// All persistent handles use `OwnedHandle` or `File`, which supply safe `Send`
/// ownership. There are no borrowed raw handles, custom unsafe `Send` impls, or
/// PID-based termination. Mutable methods serialize supervision of this owner.
/// Dropping kills job members but does not wait for their exit. Explicitly wait
/// for the process handle before reporting completion or starting a new session.
///
/// Taken pipes remain the caller's responsibility: drain both output pipes
/// concurrently and perform potentially blocking stdin writes separately from
/// the supervisor's deadline. Pipe EOF does not prove process exit.
pub struct EngineProcess {
    // Fields drop in declaration order: initiate job termination before closing
    // the process handle and our remaining pipe ends, including during unwinding.
    _job: OwnedHandle,
    process: OwnedHandle,
    stdin: Option<File>,
    stdout: Option<File>,
    stderr: Option<File>,
    id: u32,
    exit_code: Option<u32>,
}

impl EngineProcess {
    /// Launch an absolute executable path directly, with Windows CRT/Rust argv
    /// quoting. Arguments must contain no NUL; credentials must not be arguments.
    /// The child inherits the environment and working directory, only the three
    /// redirected standard handles, and no console. `STARTF_USESTDHANDLES`
    /// supplies these pipes even to a Windows GUI subsystem engine (exercised
    /// by process-probe). Successful creation does not imply successful engine
    /// initialization.
    pub fn spawn(executable: &Path, args: &[OsString]) -> io::Result<Self> {
        let (application, mut command_line) = command_line(executable, args)?;
        let job = create_job()?;
        let (stdin_read, stdin_write) = pipe(false)?;
        let (stdout_read, stdout_write) = pipe(true)?;
        let (stderr_read, stderr_write) = pipe(true)?;

        // Attribute values must stay alive until the list is deleted. Neither
        // the job nor any parent-side pipe end appears in the inheritance list.
        let jobs = [job.as_raw_handle()];
        let inherited = [
            stdin_read.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        let attributes = AttributeList::new(&jobs, &inherited)?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = inherited[0];
        startup.StartupInfo.hStdOutput = inherited[1];
        startup.StartupInfo.hStdError = inherited[2];
        startup.lpAttributeList = attributes.ptr;
        let mut info = PROCESS_INFORMATION::default();

        // SAFETY: All buffers/handles and the initialized extended attribute
        // list remain valid through this call. The command line is writable.
        // Creation-time job assignment closes the spawn/assign parent-death race.
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1, // HANDLE_LIST requires bInheritHandles=TRUE.
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                null(),
                null(),
                &startup.StartupInfo,
                &mut info,
            )
        };
        if created == 0 {
            // Capture before any cleanup can overwrite the thread's last error.
            return Err(io::Error::last_os_error());
        }

        // SAFETY: Successful CreateProcessW returns two unique owned handles.
        // No fallible work remains after creation; all cleanup is RAII-owned.
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
        let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
        drop(thread);
        drop(attributes);
        // Close the parent's copies of child-side ends immediately so EOF and
        // broken pipe reflect child lifetime rather than a leaked parent copy.
        drop(stdin_read);
        drop(stdout_write);
        drop(stderr_write);

        Ok(Self {
            _job: job,
            process,
            stdin: Some(File::from(stdin_write)),
            stdout: Some(File::from(stdout_read)),
            stderr: Some(File::from(stderr_read)),
            id: info.dwProcessId,
            exit_code: None,
        })
    }

    pub fn take_stdin(&mut self) -> Option<File> {
        self.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<File> {
        self.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<File> {
        self.stderr.take()
    }

    /// Diagnostic identity only. Supervision always uses the owned handle.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Read-only membership check against a caller's held process handle.
    /// Useful when an owned launcher creates a distinct visible application root.
    /// This never grants termination rights or supervises a process by PID.
    pub fn contains_process(&self, process: &OwnedHandle) -> io::Result<bool> {
        let mut member = 0;
        // SAFETY: both handles are held for this call and result is a live BOOL output.
        if unsafe {
            IsProcessInJob(
                process.as_raw_handle(),
                self._job.as_raw_handle(),
                &mut member,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(member != 0)
    }

    pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
        self.wait_timeout(Duration::ZERO)
    }

    /// Request forced exit (code 1), independent of any pipe progress. This is
    /// idempotent after observed exit, including a race with natural exit.
    /// Windows termination is asynchronous: follow with `wait_timeout` and keep
    /// cleanup incomplete if no exit has been observed by the caller's deadline.
    pub fn terminate(&mut self) -> io::Result<()> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        // SAFETY: We retain ownership of the real process handle.
        if unsafe { TerminateProcess(self.process.as_raw_handle(), 1) } == 0 {
            let error = io::Error::last_os_error();
            // A process which exited between the check and TerminateProcess
            // returns access denied. Only an actual signaled handle excuses it.
            if self.try_wait()?.is_none() {
                return Err(error);
            }
        }
        Ok(())
    }

    /// Wait on the real process, never EOF/status/PID. Sub-millisecond waits
    /// round up to Windows' millisecond resolution. Long durations are split
    /// into finite waits so `u32::MAX` can never become Windows' INFINITE.
    pub fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<u32>> {
        if let Some(code) = self.exit_code {
            return Ok(Some(code));
        }
        let start = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(start.elapsed());
            // SAFETY: The process handle cannot be closed while self is borrowed.
            match unsafe {
                WaitForSingleObject(self.process.as_raw_handle(), wait_millis(remaining))
            } {
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    // SAFETY: Valid owned handle and writable exit-code storage.
                    if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    // 259 (STILL_ACTIVE) is also a valid actual exit code. The
                    // signaled process handle, not a reserved code, proves exit.
                    self.exit_code = Some(code);
                    return Ok(Some(code));
                }
                WAIT_TIMEOUT if start.elapsed() >= timeout => return Ok(None),
                WAIT_TIMEOUT => continue,
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }
}

fn wait_millis(duration: Duration) -> u32 {
    duration
        .as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32
}

fn create_job() -> io::Result<OwnedHandle> {
    // SAFETY: NULL attributes create a non-inheritable handle; NULL name creates
    // a fresh unnamed job, so no other process can reopen it by a known name.
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Fresh uniquely owned handle from CreateJobObjectW.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: Valid job and correctly sized information structure.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

/// `parent_reads` chooses which end must not be inherited.
fn pipe(parent_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    // SAFETY: Writable handle slots and a valid attributes structure.
    if unsafe { CreatePipe(&mut read, &mut write, &security, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe returned two unique owned handles.
    let read = unsafe { OwnedHandle::from_raw_handle(read) };
    let write = unsafe { OwnedHandle::from_raw_handle(write) };
    let parent = if parent_reads { &read } else { &write };
    // SAFETY: We own the handle and change only its inheritance bit.
    if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

struct AttributeList<'a> {
    // u128 provides 16-byte alignment as with Windows' heap on x64. The opaque
    // list points into this fixed allocation; moving the owner does not move it.
    _storage: Box<[u128]>,
    ptr: LPPROC_THREAD_ATTRIBUTE_LIST,
    _handles: PhantomData<&'a [HANDLE]>,
}

impl<'a> AttributeList<'a> {
    fn new(jobs: &'a [HANDLE], inherited: &'a [HANDLE]) -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: NULL is the documented size query; its failure is expected.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0u128; bytes.div_ceil(size_of::<u128>())].into_boxed_slice();
        let ptr = storage.as_mut_ptr().cast();
        // SAFETY: Stable, aligned allocation of at least the requested size.
        if unsafe { InitializeProcThreadAttributeList(ptr, 2, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self {
            _storage: storage,
            ptr,
            _handles: PhantomData,
        };
        list.set_handles(PROC_THREAD_ATTRIBUTE_JOB_LIST, jobs)?;
        list.set_handles(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, inherited)?;
        Ok(list)
    }

    fn set_handles(&mut self, attribute: u32, handles: &'a [HANDLE]) -> io::Result<()> {
        // SAFETY: Initialized list; callers keep the handle slice alive until
        // this list is deleted, including on any subsequent failure path.
        if unsafe {
            UpdateProcThreadAttribute(
                self.ptr,
                0,
                attribute as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: List was initialized successfully; backing storage is alive.
        unsafe { DeleteProcThreadAttributeList(self.ptr) };
    }
}

fn command_line(executable: &Path, args: &[OsString]) -> io::Result<(Vec<u16>, Vec<u16>)> {
    if !executable.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "executable must be absolute",
        ));
    }
    let mut application = wide(executable.as_os_str())?;
    if application.contains(&(b'"' as u16)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid executable path",
        ));
    }
    // argv[0] has special parsing rules: quote the path literally. Backslash
    // escaping applies only to later arguments (CRT and Rust's Windows parser).
    let mut command = vec![b'"' as u16];
    command.extend_from_slice(&application);
    command.push(b'"' as u16);
    for arg in args {
        command.push(b' ' as u16);
        quote_arg(&wide(arg)?, &mut command);
        if command.len() >= 32_767 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "command line too long",
            ));
        }
    }
    if command.len() >= 32_767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command line too long",
        ));
    }
    application.push(0);
    command.push(0);
    Ok((application, command))
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in process argument",
        ));
    }
    Ok(value)
}

fn quote_arg(arg: &[u16], command: &mut Vec<u16>) {
    command.push(b'"' as u16);
    let mut slashes = 0;
    for &unit in arg {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let count = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        command.extend(std::iter::repeat_n(b'\\' as u16, count));
        command.push(unit);
        slashes = 0;
    }
    command.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    command.push(b'"' as u16);
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::GetHandleInformation;

    #[test]
    fn ownership_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<EngineProcess>();
    }

    #[test]
    fn waits_are_finite_and_round_up() {
        assert_eq!(wait_millis(Duration::ZERO), 0);
        assert_eq!(wait_millis(Duration::from_nanos(1)), 1);
        assert_eq!(wait_millis(Duration::from_micros(1001)), 2);
        assert_eq!(wait_millis(Duration::MAX), u32::MAX - 1);
    }

    #[test]
    fn invalid_inputs_fail_before_launch() {
        for (path, args) in [
            (Path::new("relative.exe"), vec![]),
            (Path::new("C:\\bad\0.exe"), vec![]),
            (Path::new("C:\\bad\".exe"), vec![]),
            (
                Path::new("C:\\unused.exe"),
                vec![OsString::from("bad\0arg")],
            ),
            (
                Path::new("C:\\unused.exe"),
                vec![OsString::from("x".repeat(32_767))],
            ),
        ] {
            assert_eq!(
                EngineProcess::spawn(path, &args).err().unwrap().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn job_and_parent_pipe_handles_are_not_inheritable() {
        fn inheritable(handle: &OwnedHandle) -> bool {
            let mut flags = 0;
            assert_ne!(
                unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) },
                0
            );
            flags & HANDLE_FLAG_INHERIT != 0
        }
        assert!(!inheritable(&create_job().unwrap()));
        let (read, write) = pipe(true).unwrap();
        assert!(!inheritable(&read));
        assert!(inheritable(&write));
        let (read, write) = pipe(false).unwrap();
        assert!(inheritable(&read));
        assert!(!inheritable(&write));
    }
}
