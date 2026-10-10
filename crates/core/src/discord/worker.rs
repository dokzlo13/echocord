#[cfg(windows)]
use crate::process::EngineProcess;
#[cfg(windows)]
use std::os::windows::io::{AsHandle, OwnedHandle};
#[cfg(not(windows))]
type OwnedHandle = ();
#[cfg(not(windows))]
struct EngineProcess(std::process::Child);
#[cfg(not(windows))]
impl EngineProcess {
    fn spawn_with_handles(
        executable: &std::path::Path,
        args: &[std::ffi::OsString],
        _: &[()],
    ) -> std::io::Result<Self> {
        use std::process::{Command, Stdio};
        Command::new(executable)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(Self)
    }
    fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.0.stdin.take()
    }
    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.0.stdout.take()
    }
    fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.0.stderr.take()
    }
    fn try_wait(&mut self) -> std::io::Result<Option<u32>> {
        self.0
            .try_wait()
            .map(|exit| exit.map(|s| s.code().unwrap_or(1) as u32))
    }
    fn terminate(&mut self) -> std::io::Result<()> {
        self.0.kill()
    }
}
#[cfg(not(windows))]
impl Drop for EngineProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
use crate::protocol::{self, Frame, Validator};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    io::{BufReader, Read},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

const QUEUE_LIMIT: usize = 64;
struct Mail<C> {
    commands: VecDeque<C>,
    stop_at: Option<Instant>,
}
pub struct Worker<C, S> {
    mail: Arc<(Mutex<Mail<C>>, Condvar)>,
    statuses: Arc<Mutex<VecDeque<S>>>,
    pub exit: Arc<AtomicU8>,
    pub started: Instant,
}
impl<C, S> Worker<C, S>
where
    C: Serialize + Send + 'static,
    S: DeserializeOwned + Send + 'static,
{
    pub fn launch(
        executable: PathBuf,
        mode: &'static str,
        generation: u32,
        handles: Vec<OwnedHandle>,
        initial: C,
        stop: C,
    ) -> Self {
        let mail = Arc::new((
            Mutex::new(Mail {
                commands: VecDeque::new(),
                stop_at: None,
            }),
            Condvar::new(),
        ));
        let statuses = Arc::new(Mutex::new(VecDeque::new()));
        let exit = Arc::new(AtomicU8::new(0));
        let worker = Self {
            mail: mail.clone(),
            statuses: statuses.clone(),
            exit: exit.clone(),
            started: Instant::now(),
        };
        std::thread::spawn(move || {
            #[cfg(windows)]
            let borrowed: Vec<_> = handles.iter().map(AsHandle::as_handle).collect();
            #[cfg(not(windows))]
            let borrowed = handles.clone();
            let Ok(mut process) =
                EngineProcess::spawn_with_handles(&executable, &[mode.into()], &borrowed)
            else {
                exit.store(1, Ordering::Release);
                return;
            };
            drop(handles);
            let (Some(mut stdin), Some(stdout), Some(mut stderr)) = (
                process.take_stdin(),
                process.take_stdout(),
                process.take_stderr(),
            ) else {
                stop_mail(&mail);
                let _ = process.terminate();
                while process.try_wait().ok().flatten().is_none() {
                    exit.store(2, Ordering::Release);
                    std::thread::sleep(Duration::from_millis(10));
                }
                exit.store(1, Ordering::Release);
                return;
            };
            let writer_mail = mail.clone();
            std::thread::spawn(move || {
                let (lock, wake) = &*writer_mail;
                if lock
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .stop_at
                    .is_some()
                {
                    return;
                }
                let mut sequence = 1;
                if protocol::write(
                    &mut stdin,
                    &Frame {
                        version: protocol::VERSION,
                        generation,
                        sequence,
                        body: initial,
                    },
                )
                .is_err()
                {
                    stop_mail(&writer_mail);
                    return;
                }
                let mut stop = Some(stop);
                loop {
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while state.commands.is_empty() && state.stop_at.is_none() {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    let stopping = state.stop_at.is_some();
                    let body = if stopping {
                        state.commands.clear();
                        stop.take().expect("one stop")
                    } else {
                        state.commands.pop_front().expect("queued command")
                    };
                    drop(state);
                    sequence += 1;
                    if protocol::write(
                        &mut stdin,
                        &Frame {
                            version: protocol::VERSION,
                            generation,
                            sequence,
                            body,
                        },
                    )
                    .is_err()
                    {
                        stop_mail(&writer_mail);
                        break;
                    }
                    if stopping {
                        break;
                    }
                }
            });
            let reader_mail = mail.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let Ok(mut validator) = Validator::new(generation) else {
                    stop_mail(&reader_mail);
                    return;
                };
                while let Ok(Some(frame)) = protocol::read::<S>(&mut reader) {
                    if validator.accept(&frame).is_err() {
                        break;
                    }
                    let mut queue = statuses.lock().unwrap_or_else(|e| e.into_inner());
                    if queue.len() == QUEUE_LIMIT {
                        break;
                    }
                    queue.push_back(frame.body);
                }
                stop_mail(&reader_mail);
            });
            std::thread::spawn(move || {
                let mut bytes = [0; 4096];
                while stderr.read(&mut bytes).is_ok_and(|n| n > 0) {}
            });
            let mut killed = false;
            loop {
                match process.try_wait() {
                    Ok(Some(_)) => {
                        stop_mail(&mail);
                        exit.store(1, Ordering::Release);
                        break;
                    }
                    Err(_) => stop_mail(&mail),
                    _ => {}
                }
                if let Some(at) = mail.0.lock().unwrap_or_else(|e| e.into_inner()).stop_at {
                    if at.elapsed() > Duration::from_millis(750) && !killed {
                        let _ = process.terminate();
                        killed = true;
                    }
                    if at.elapsed() > Duration::from_millis(3750) {
                        let _ = exit.compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        worker
    }
    pub fn send(&self, command: C) -> bool {
        let (lock, wake) = &*self.mail;
        let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
        if mail.stop_at.is_some() {
            return false;
        }
        if mail.commands.len() == QUEUE_LIMIT {
            mail.stop_at = Some(Instant::now());
            wake.notify_one();
            return false;
        }
        mail.commands.push_back(command);
        wake.notify_one();
        true
    }
    pub fn take_statuses(&self) -> Vec<S> {
        self.statuses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }
    pub fn stop(&self) {
        stop_mail(&self.mail);
    }
    pub fn stopped_at(&self) -> Option<Instant> {
        self.mail
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop_at
    }
    pub fn poll_deadline(&self) {
        if self
            .stopped_at()
            .is_some_and(|t| t.elapsed() > Duration::from_secs(4))
        {
            let _ = self
                .exit
                .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}
impl<C, S> Drop for Worker<C, S> {
    fn drop(&mut self) {
        stop_mail(&self.mail);
    }
}
fn stop_mail<C>(mail: &Arc<(Mutex<Mail<C>>, Condvar)>) {
    let (lock, wake) = &**mail;
    let mut mail = lock.lock().unwrap_or_else(|e| e.into_inner());
    mail.stop_at.get_or_insert_with(Instant::now);
    mail.commands.clear();
    wake.notify_one();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::discord::protocol::{GatewayCommand, GatewayStatus};
    use std::os::unix::fs::PermissionsExt;
    fn fixture(name: &str, script: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "echocord-room-worker-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("worker.py");
        std::fs::write(&path, format!("#!/usr/bin/python3\n{script}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn wait_exit(worker: &Worker<GatewayCommand, GatewayStatus>) {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until && worker.exit.load(Ordering::Acquire) != 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(worker.exit.load(Ordering::Acquire), 1);
    }
    #[test]
    fn blocked_private_pipe_cannot_delay_kill_or_observed_exit() {
        let path = fixture(
            "blocked",
            "import os,time\nos.write(2,b'x'*65536)\nwhile True: time.sleep(1)\n",
        );
        let worker = Worker::<_, GatewayStatus>::launch(
            path.clone(),
            "--gateway-worker",
            1,
            vec![],
            GatewayCommand::Init {
                token_file: PathBuf::from("x".repeat(32000)),
            },
            GatewayCommand::Stop,
        );
        std::thread::sleep(Duration::from_millis(30));
        let before = Instant::now();
        worker.stop();
        assert!(before.elapsed() < Duration::from_millis(100));
        assert!(!worker.send(GatewayCommand::Stop));
        wait_exit(&worker);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn eof_is_a_stop_request_and_exit_still_requires_native_receipt() {
        let path = fixture(
            "eof",
            "import os,time\nos.close(1)\nwhile True: time.sleep(1)\n",
        );
        let worker = Worker::<_, GatewayStatus>::launch(
            path.clone(),
            "--gateway-worker",
            1,
            vec![],
            GatewayCommand::Stop,
            GatewayCommand::Stop,
        );
        let until = Instant::now() + Duration::from_secs(1);
        while worker.stopped_at().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(worker.stopped_at().is_some());
        assert_eq!(worker.exit.load(Ordering::Acquire), 0);
        wait_exit(&worker);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn flooding_statuses_is_bounded_and_contains_only_that_worker() {
        let path = fixture(
            "flood",
            "import os,json,time\nfor i in range(200):\n os.write(1,(json.dumps({'version':1,'generation':7,'sequence':i+1,'body':{'type':'ready'}})+'\\n').encode())\nwhile True: time.sleep(1)\n",
        );
        let worker = Worker::<_, GatewayStatus>::launch(
            path.clone(),
            "--gateway-worker",
            7,
            vec![],
            GatewayCommand::Stop,
            GatewayCommand::Stop,
        );
        wait_exit(&worker);
        assert_eq!(worker.take_statuses().len(), QUEUE_LIMIT);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
