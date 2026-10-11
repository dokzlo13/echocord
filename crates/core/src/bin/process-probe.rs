//! Synthetic Windows supervision checks; no credentials, audio, or network.
//! Run from the checkout, optionally passing an ignored evidence directory.
//! Child modes are internal to this probe and must run before any application UI.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "../process.rs"]
mod process;

#[cfg(windows)]
mod probe {
    use super::process::EngineProcess;
    use std::{
        ffi::OsString,
        fs::{self, File},
        io::{self, BufRead, BufReader, Read, Write},
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr::{null, null_mut},
        sync::mpsc::{self, Receiver},
        thread::{self, JoinHandle},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::SECURITY_ATTRIBUTES,
        System::{
            JobObjects::IsProcessInJob,
            Threading::{
                CREATE_NO_WINDOW, CreateEventW, ExitProcess, GetCurrentProcess,
                GetProcessHandleCount, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, SetEvent, TerminateProcess,
                WaitForSingleObject,
            },
        },
    };

    // Probe-only declaration avoids expanding the coordinator-owned feature
    // list just to observe console absence. GetConsoleWindow is in Kernel32.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleWindow() -> HWND;
    }

    const DEADLINE: Duration = Duration::from_secs(5);
    const QUIET_WAIT: Duration = Duration::from_millis(150);

    fn check(condition: bool, message: &'static str) -> io::Result<()> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message))
        }
    }

    fn park() -> ! {
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    fn worker(args: &[OsString], expect_job: bool) -> io::Result<()> {
        // The ownership job must already exist before the worker's first action.
        let mut in_job = 0;
        if expect_job {
            check(
                unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut in_job) } != 0
                    && in_job != 0,
                "worker started outside a job",
            )?;
        }
        check(
            unsafe { GetConsoleWindow() }.is_null(),
            "worker has a console",
        )?;
        let mode = args.first().and_then(|s| s.to_str()).unwrap_or("");
        match mode {
            "exit" => {
                let code: u32 = args[1]
                    .to_string_lossy()
                    .parse()
                    .map_err(io::Error::other)?;
                unsafe { ExitProcess(code) }
            }
            "argv" => {
                let mut out = io::stdout().lock();
                for arg in &args[1..] {
                    let units: Vec<_> = arg.encode_wide().collect();
                    out.write_all(&(units.len() as u32).to_le_bytes())?;
                    for unit in units {
                        out.write_all(&unit.to_le_bytes())?;
                    }
                }
            }
            "handle" => {
                let value: usize = args[1]
                    .to_string_lossy()
                    .parse()
                    .map_err(io::Error::other)?;
                let inherited = unsafe { SetEvent(value as HANDLE) } != 0;
                check(!inherited, "unlisted event was inherited")?;
            }
            "stop" => {
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                check(line == "Stop\n", "wrong stop command")?;
            }
            "ignore-stop" => {
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                check(line == "Stop\n", "wrong stop command")?;
                println!("IGNORED");
                io::stdout().flush()?;
                park();
            }
            "false-stop" => {
                println!("Stopped");
                io::stdout().flush()?;
                // Deliberate synthetic pipe EOF while remaining alive. These
                // global standard handles will never be used again in this mode.
                unsafe {
                    CloseHandle(io::stdin().as_raw_handle());
                    CloseHandle(io::stdout().as_raw_handle());
                    CloseHandle(io::stderr().as_raw_handle());
                }
                park();
            }
            "flood" => {
                thread::spawn(|| {
                    let mut out = io::stdout().lock();
                    let chunk = [b'o'; 4096];
                    while out.write_all(&chunk).is_ok() {}
                });
                thread::spawn(|| {
                    let mut out = io::stderr().lock();
                    let chunk = [b'e'; 4096];
                    while out.write_all(&chunk).is_ok() {}
                });
                // Never read stdin: a large parent write must block while both
                // output drainers and the independent termination deadline run.
                park();
            }
            "startup" => {
                // No initialization has been received until this gate opens.
                println!("STARTUP");
                io::stdout().flush()?;
                let mut line = String::new();
                // EOF, Stop, or a read error must never voluntarily end this
                // fixture: parent-death checks must depend on job closure.
                let _ = io::stdin().read_line(&mut line);
                if line == "Initialize\n" {
                    println!("ACTIVE");
                    io::stdout().flush()?;
                }
                park();
            }
            _ => return Err(io::Error::other("unknown worker mode")),
        }
        Ok(())
    }

    /// Opens the real process before ownership is lost. Failure cleanup kills a
    /// surviving test process, but only after assertions observed the outcome.
    struct Observer(OwnedHandle);

    impl Observer {
        fn open(id: u32) -> io::Result<Self> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    id,
                )
            };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
        }

        fn assert_alive(&self) -> io::Result<()> {
            check(
                unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) } == WAIT_TIMEOUT,
                "worker was not alive before ownership loss",
            )
        }

        fn assert_exited(&self) -> io::Result<()> {
            check(
                unsafe { WaitForSingleObject(self.0.as_raw_handle(), DEADLINE.as_millis() as u32) }
                    == WAIT_OBJECT_0,
                "orphan remained after ownership loss",
            )
        }

        fn terminate(&self) -> io::Result<()> {
            if unsafe { TerminateProcess(self.0.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            self.assert_exited()
        }
    }

    impl Drop for Observer {
        fn drop(&mut self) {
            if unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) } == WAIT_TIMEOUT {
                unsafe {
                    TerminateProcess(self.0.as_raw_handle(), 1);
                    WaitForSingleObject(self.0.as_raw_handle(), DEADLINE.as_millis() as u32);
                }
            }
        }
    }

    fn spawn(exe: &Path, mode: &str, more: &[OsString]) -> io::Result<EngineProcess> {
        let mut args = vec![OsString::from("--worker"), OsString::from(mode)];
        args.extend_from_slice(more);
        EngineProcess::spawn(exe, &args)
    }

    fn exited(child: &mut EngineProcess, code: u32) -> io::Result<()> {
        check(
            child.wait_timeout(DEADLINE)? == Some(code),
            "wrong or unconfirmed exit code",
        )?;
        check(
            child.try_wait()? == Some(code),
            "exit code was not retained",
        )?;
        child.terminate()?;
        check(
            child.try_wait()? == Some(code),
            "terminate changed an observed exit",
        )
    }

    fn force_exit(child: &mut EngineProcess) -> io::Result<()> {
        child.terminate()?;
        exited(child, 1)
    }

    fn receive<T>(rx: &Receiver<T>) -> io::Result<T> {
        rx.recv_timeout(DEADLINE)
            .map_err(|_| io::Error::other("probe operation timed out"))
    }

    struct Drain {
        progress: Receiver<u64>,
        done: Receiver<io::Result<u64>>,
        thread: JoinHandle<()>,
    }

    impl Drain {
        fn start(mut file: File) -> Self {
            let (progress_tx, progress) = mpsc::sync_channel(1);
            let (done_tx, done) = mpsc::channel();
            let thread = thread::spawn(move || {
                let result = (|| {
                    let mut count = 0;
                    let mut chunk = [0; 8192];
                    loop {
                        let read = file.read(&mut chunk)?;
                        if read == 0 {
                            return Ok(count);
                        }
                        count += read as u64;
                        if count >= 128 * 1024 {
                            let _ = progress_tx.try_send(count);
                        }
                    }
                })();
                let _ = done_tx.send(result);
            });
            Self {
                progress,
                done,
                thread,
            }
        }

        fn finish(self) -> io::Result<u64> {
            let count = receive(&self.done)??;
            self.thread
                .join()
                .map_err(|_| io::Error::other("drainer panicked"))?;
            Ok(count)
        }
    }

    fn read_all(mut file: File) -> io::Result<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut bytes = vec![];
            let result = file.read_to_end(&mut bytes).map(|_| bytes);
            let _ = tx.send(result);
        });
        let bytes = receive(&rx)??;
        thread
            .join()
            .map_err(|_| io::Error::other("reader panicked"))?;
        Ok(bytes)
    }

    fn read_line<R: Read + Send + 'static>(file: R) -> io::Result<String> {
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(file).read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
        });
        let line = receive(&rx)??;
        thread
            .join()
            .map_err(|_| io::Error::other("line reader panicked"))?;
        Ok(line)
    }

    fn quoting(exe: &Path, evidence: &Path) -> io::Result<()> {
        // Copy to a path with spaces and Unicode to test lpApplicationName and
        // argv[0] as well as argument escaping; the copy remains inside the selected evidence directory.
        let copy = evidence.join("quoted executable 音.exe");
        fs::copy(exe, &copy)?;
        let args: Vec<OsString> = [
            "",
            "plain",
            "two words",
            "\t",
            "a\"b",
            "\\",
            "ends\\",
            "two slashes\\\\",
            "\\\"",
            "a\\\\\"b",
            " space at end ",
            "&|<>^%PATH%!",
            "音🎵",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([
            // OsString is allowed to contain unpaired UTF-16 surrogates.
            <OsString as std::os::windows::ffi::OsStringExt>::from_wide(&[0xd800, b'x' as u16]),
        ])
        .collect();
        let mut child = spawn(&copy, "argv", &args)?;
        let out = child.take_stdout().unwrap();
        check(child.take_stdout().is_none(), "stdout was taken twice")?;
        exited(&mut child, 0)?;
        let bytes = read_all(out)?;
        let mut expected = vec![];
        for arg in args {
            let units: Vec<_> = arg.encode_wide().collect();
            expected.extend_from_slice(&(units.len() as u32).to_le_bytes());
            for unit in units {
                expected.extend_from_slice(&unit.to_le_bytes());
            }
        }
        check(bytes == expected, "arguments did not survive exact launch")
    }

    fn inheritance(exe: &Path) -> io::Result<()> {
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let raw = unsafe { CreateEventW(&security, 1, 0, null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let sentinel = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut child = spawn(exe, "handle", &[OsString::from((raw as usize).to_string())])?;
        exited(&mut child, 0)?;
        check(
            unsafe { WaitForSingleObject(sentinel.as_raw_handle(), 0) } == WAIT_TIMEOUT,
            "unlisted inheritable event reached worker",
        )
    }

    fn graceful_and_ignored(exe: &Path) -> io::Result<()> {
        let mut child = spawn(exe, "stop", &[])?;
        let observer = Observer::open(child.id())?;
        check(
            child.contains_process(&observer.0)?,
            "worker was not in its owned job",
        )?;
        child.take_stdin().unwrap().write_all(b"Stop\n")?;
        check(child.take_stdin().is_none(), "stdin was taken twice")?;
        exited(&mut child, 0)?;

        let mut child = spawn(exe, "ignore-stop", &[])?;
        child.take_stdin().unwrap().write_all(b"Stop\n")?;
        check(
            read_line(child.take_stdout().unwrap())? == "IGNORED\n",
            "stop was not received",
        )?;
        check(
            child.wait_timeout(QUIET_WAIT)?.is_none(),
            "ignored stop claimed exit",
        )?;
        force_exit(&mut child)
    }

    fn false_completion(exe: &Path) -> io::Result<()> {
        let mut child = spawn(exe, "false-stop", &[])?;
        check(
            read_all(child.take_stdout().unwrap())? == b"Stopped\n",
            "missing false stop",
        )?;
        check(
            read_all(child.take_stderr().unwrap())?.is_empty(),
            "unexpected stderr",
        )?;
        check(
            child.try_wait()?.is_none(),
            "pipe EOF falsely confirmed exit",
        )?;
        force_exit(&mut child)
    }

    fn blocked_and_flooding(exe: &Path) -> io::Result<()> {
        let mut child = spawn(exe, "flood", &[])?;
        let stdout = Drain::start(child.take_stdout().unwrap());
        let stderr = Drain::start(child.take_stderr().unwrap());
        check(child.take_stderr().is_none(), "stderr was taken twice")?;
        receive(&stdout.progress)?;
        receive(&stderr.progress)?;
        let mut stdin = child.take_stdin().unwrap();
        let (started_tx, started) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let payload = vec![b'i'; 8 * 1024 * 1024];
            let _ = started_tx.send(());
            let _ = tx.send(stdin.write_all(&payload));
        });
        receive(&started)?;
        check(
            matches!(
                rx.recv_timeout(QUIET_WAIT),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "stdin write did not block",
        )?;
        check(
            child.try_wait()?.is_none(),
            "flood worker exited prematurely",
        )?;
        let start = Instant::now();
        force_exit(&mut child)?;
        check(
            start.elapsed() < DEADLINE,
            "termination was blocked by pipe I/O",
        )?;
        check(
            receive(&rx)?.is_err(),
            "blocked stdin did not break after exit",
        )?;
        writer
            .join()
            .map_err(|_| io::Error::other("writer panicked"))?;
        check(
            stdout.finish()? >= 128 * 1024,
            "stdout flood was not drained",
        )?;
        check(
            stderr.finish()? >= 128 * 1024,
            "stderr flood was not drained",
        )
    }

    fn early_exit(exe: &Path) -> io::Result<()> {
        // Include the Win32 STILL_ACTIVE value and an unsigned high-bit code.
        for code in [0, 37, 259, 0xe000_0001] {
            let mut child = spawn(exe, "exit", &[OsString::from(code.to_string())])?;
            exited(&mut child, code)?;
            check(
                read_all(child.take_stdout().unwrap())?.is_empty(),
                "early exit emitted output",
            )?;
            check(
                read_all(child.take_stderr().unwrap())?.is_empty(),
                "early exit emitted error",
            )?;
            check(
                child
                    .take_stdin()
                    .unwrap()
                    .write_all(b"Initialize\n")
                    .is_err(),
                "stdin end leaked",
            )?;
        }
        Ok(())
    }

    fn drop_ownership(exe: &Path) -> io::Result<()> {
        for _ in 0..16 {
            let child = spawn(exe, "startup", &[])?;
            let observer = Observer::open(child.id())?;
            // No worker readiness or initialization receipt precedes this drop.
            observer.assert_alive()?;
            drop(child);
            observer.assert_exited()?;
        }
        let mut child = spawn(exe, "startup", &[])?;
        let observer = Observer::open(child.id())?;
        check(
            read_line(child.take_stdout().unwrap())? == "STARTUP\n",
            "startup not reached",
        )?;
        observer.assert_alive()?;
        // Exercise Send ownership by dropping on another thread, with stdin
        // held outside the owner. That pipe must not keep the job alive.
        let stdin = child.take_stdin().unwrap();
        thread::spawn(move || drop(child))
            .join()
            .map_err(|_| io::Error::other("owner panicked"))?;
        observer.assert_exited()?;
        drop(stdin);

        let mut child = spawn(exe, "startup", &[])?;
        let observer = Observer::open(child.id())?;
        check(
            read_line(child.take_stdout().unwrap())? == "STARTUP\n",
            "startup not reached",
        )?;
        let panic = thread::spawn(move || {
            let _owner = child;
            // Suppress the expected panic report without changing the main
            // process hook: unwind directly without invoking the hook.
            std::panic::resume_unwind(Box::new("synthetic owner unwind"));
        })
        .join();
        check(panic.is_err(), "owner did not unwind")?;
        observer.assert_exited()
    }

    fn owner(phase: &str) -> io::Result<()> {
        let exe = std::env::current_exe()?;
        if phase == "unowned" {
            // A control experiment using the same EOF-ignoring fixture without
            // our job. std::Child does not kill the child on owner destruction.
            let mut child = Command::new(&exe)
                .args(["--unowned-worker", "startup"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let mut out = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            out.read_line(&mut line)?;
            check(line == "STARTUP\n", "unowned worker never started")?;
            println!("{}", child.id());
            io::stdout().flush()?;
            park();
        }
        check(
            matches!(phase, "created" | "startup" | "active"),
            "unknown owner phase",
        )?;
        let mut child = spawn(&exe, "startup", &[])?;
        if phase == "created" {
            println!("{}", child.id());
            io::stdout().flush()?;
            park();
        }
        let mut out = BufReader::new(child.take_stdout().unwrap());
        let mut line = String::new();
        out.read_line(&mut line)?;
        check(line == "STARTUP\n", "worker initialization barrier failed")?;
        if phase == "active" {
            child.take_stdin().unwrap().write_all(b"Initialize\n")?;
            line.clear();
            out.read_line(&mut line)?;
            check(line == "ACTIVE\n", "worker never became active")?;
        }
        println!("{}", child.id());
        io::stdout().flush()?;
        // The outside observer will use TerminateProcess on this owner. No
        // destructors execute: only kernel handle closure can kill the worker.
        park();
    }

    struct OutsideOwner(Child);

    impl Drop for OutsideOwner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            // Bound even failure cleanup; never let a broken probe hang forever.
            unsafe {
                WaitForSingleObject(self.0.as_raw_handle(), DEADLINE.as_millis() as u32);
            }
        }
    }

    fn parent_death(exe: &Path, phase: &str, evidence: &Path) -> io::Result<()> {
        // Intentionally launch the owner WITHOUT EngineProcess/a supervisor job.
        // Otherwise the observer's own nested job could mask an orphan bug.
        let log = File::create(evidence.join(format!("owner-{phase}-stderr.log")))?;
        let mut owner = OutsideOwner(
            Command::new(exe)
                .args(["--owner", phase])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        let line = read_line(owner.0.stdout.take().unwrap())?;
        let id: u32 = line.trim().parse().map_err(io::Error::other)?;
        fs::write(evidence.join(format!("worker-{phase}.pid")), &line)?;
        let worker = Observer::open(id)?;
        worker.assert_alive()?;
        owner.0.kill()?;
        check(
            unsafe { WaitForSingleObject(owner.0.as_raw_handle(), DEADLINE.as_millis() as u32) }
                == WAIT_OBJECT_0,
            "abrupt parent exit not observed",
        )?;
        if phase == "unowned" {
            thread::sleep(QUIET_WAIT);
            worker.assert_alive()?;
            worker.terminate()
        } else {
            worker.assert_exited()
        }
    }

    fn handles() -> io::Result<u32> {
        let mut count = 0;
        if unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(count)
    }

    fn concurrent_launch(exe: &Path) -> io::Result<()> {
        let mut workers = vec![];
        for _ in 0..4 {
            let exe = exe.to_owned();
            workers.push(thread::spawn(move || -> io::Result<()> {
                for _ in 0..8 {
                    let mut child = spawn(&exe, "stop", &[])?;
                    child.take_stdin().unwrap().write_all(b"Stop\n")?;
                    exited(&mut child, 0)?;
                    check(
                        read_all(child.take_stdout().unwrap())?.is_empty(),
                        "concurrent launch retained a stdout writer",
                    )?;
                    check(
                        read_all(child.take_stderr().unwrap())?.is_empty(),
                        "concurrent launch retained a stderr writer",
                    )?;
                }
                Ok(())
            }));
        }
        // Join every worker even if an earlier one failed, so the probe has no
        // detached launcher left when subsequent lifetime checks begin.
        let mut outcome = Ok(());
        for worker in workers {
            let result = worker
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("launcher panicked")));
            if outcome.is_ok() {
                outcome = result;
            }
        }
        outcome
    }

    fn repeated_launch(exe: &Path, evidence: &Path) -> io::Result<()> {
        let mut counts = File::create(evidence.join("handle-counts.log"))?;
        writeln!(counts, "before-failed-create-warmup {}", handles()?)?;
        // Windows may lazily initialize process-creation failure handling. Use
        // the platform's plain process launcher first to distinguish that cost
        // from owned-job/pipe leaks; then warm both paths of this abstraction.
        check(
            Command::new(evidence.join("platform-missing.exe"))
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_err(),
            "platform missing executable launched",
        )?;
        writeln!(counts, "after-platform-failed-create {}", handles()?)?;
        check(
            EngineProcess::spawn(&evidence.join("owned-missing.exe"), &[]).is_err(),
            "owned missing executable launched",
        )?;
        writeln!(counts, "after-owned-failed-create {}", handles()?)?;
        // Warm up std/thread/process bookkeeping before comparing handles.
        let mut child = spawn(exe, "exit", &[OsString::from("0")])?;
        exited(&mut child, 0)?;
        drop(child);
        let before = handles()?;
        writeln!(counts, "baseline {before}")?;
        for _ in 0..32 {
            let mut child = spawn(exe, "exit", &[OsString::from("0")])?;
            exited(&mut child, 0)?;
            drop(child);
            writeln!(counts, "success {}", handles()?)?;
        }
        for attempt in 0..32 {
            check(
                EngineProcess::spawn(&evidence.join(format!("missing-{attempt}.exe")), &[])
                    .is_err(),
                "missing executable launched",
            )?;
            writeln!(counts, "failure {}", handles()?)?;
        }
        check(
            handles()? <= before,
            "repeated successful/failed launch leaked handles",
        )
    }

    pub fn run() -> io::Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        match args.first().and_then(|s| s.to_str()) {
            Some("--worker") => return worker(&args[1..], true),
            Some("--unowned-worker") => return worker(&args[1..], false),
            Some("--owner") => return owner(&args[1].to_string_lossy()),
            _ => {}
        }
        check(
            args.len() <= 1,
            "usage: process-probe [ignored-evidence-directory]",
        )?;
        let exe = std::env::current_exe()?;
        let root = args.first().map(PathBuf::from).unwrap_or(
            std::env::current_dir()?
                .join("artifacts")
                .join("process-probe"),
        );
        fs::create_dir_all(&root)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let evidence = root.join(format!("run-{stamp}"));
        fs::create_dir(&evidence)?;

        quoting(&exe, &evidence)?;
        println!("PASS: exact executable and argument quoting; worker has no console");
        inheritance(&exe)?;
        println!("PASS: unrelated inheritable handles excluded");
        graceful_and_ignored(&exe)?;
        println!("PASS: graceful stop and ignored stop with forced exit confirmation");
        false_completion(&exe)?;
        println!("PASS: Stopped and both pipe EOFs do not prove process exit");
        blocked_and_flooding(&exe)?;
        println!("PASS: blocked stdin and concurrent output floods do not block termination");
        early_exit(&exe)?;
        println!("PASS: initialization exit and all unsigned exit codes observed");
        drop_ownership(&exe)?;
        println!("PASS: drop across threads and panic unwind leave no worker");
        parent_death(&exe, "unowned", &evidence)?;
        println!(
            "PASS: control worker survives parent death without its own job, then is explicitly cleaned up"
        );
        for phase in ["created", "startup", "active"] {
            for attempt in 0..4 {
                let case = evidence.join(format!("{phase}-{attempt}"));
                fs::create_dir(&case)?;
                parent_death(&exe, phase, &case)?;
            }
        }
        println!(
            "PASS: repeated abrupt parent death after creation, during startup, and active leaves no worker"
        );
        concurrent_launch(&exe)?;
        println!("PASS: concurrent launches inherit only their own pipes");
        repeated_launch(&exe, &evidence)?;
        println!("PASS: next launches succeed; repeated success/failure leaks no handles");
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    probe::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("process-probe requires native Windows execution");
    std::process::exit(1);
}
