//! One disk writer, one active revision and one coalesced pending setup.
//! The actor owns scheduling; the worker never holds an actor lock during I/O.
use super::{AppError, ControllerConfig, ErrorCode, Preferences, error, write_preferences};
use std::{
    sync::{Arc, mpsc},
    thread::JoinHandle,
    time::Duration,
};
use tokio::time::Instant;

const DEBOUNCE: Duration = Duration::from_millis(150);
const MAX_WAIT: Duration = Duration::from_millis(500);
type Store = Arc<dyn Fn(&ControllerConfig, &Preferences) -> Result<(), AppError> + Send + Sync>;

struct Job {
    revision: u64,
    config: ControllerConfig,
    preferences: Preferences,
}

struct Pending {
    job: Job,
    first_requested: Instant,
    due: Instant,
    urgent: bool,
}
pub(super) struct Completion {
    pub revision: u64,
    pub result: Result<(), AppError>,
}
struct Worker {
    jobs: Option<mpsc::SyncSender<Job>>,
    results: mpsc::Receiver<Completion>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn spawn(store: Store) -> Result<Self, AppError> {
        let (jobs, requests) = mpsc::sync_channel::<Job>(1);
        let (completed, results) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("preferences-writer".into())
            .spawn(move || {
                while let Ok(job) = requests.recv() {
                    let result = store(&job.config, &job.preferences);
                    if completed
                        .send(Completion {
                            revision: job.revision,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|_| writer_failed())?;
        Ok(Self {
            jobs: Some(jobs),
            results,
            thread: Some(thread),
        })
    }
    fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.jobs.take();
        // Normal shutdown observes completion and thread exit first. Never
        // block the control actor joining an outstanding OS disk operation.
        if self.finished()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}
fn writer_failed() -> AppError {
    error(
        ErrorCode::Configuration,
        "Preferences could not be saved. Retry saving the current setup.",
    )
}

pub(super) struct PreferencesWriter {
    store: Store,
    worker: Option<Worker>,
    pending: Option<Pending>,
    active: Option<u64>,
    latest_revision: u64,
    dirty: bool,
    closing: bool,
}
impl Default for PreferencesWriter {
    fn default() -> Self {
        Self::with_store(Arc::new(write_preferences))
    }
}
impl PreferencesWriter {
    fn with_store(store: Store) -> Self {
        Self {
            store,
            worker: None,
            pending: None,
            active: None,
            latest_revision: 0,
            dirty: false,
            closing: false,
        }
    }
    pub fn request(
        &mut self,
        config: ControllerConfig,
        preferences: Preferences,
        now: Instant,
        urgent: bool,
    ) -> Result<(), AppError> {
        if self.closing {
            return Err(writer_failed());
        }
        self.latest_revision = self
            .latest_revision
            .checked_add(1)
            .ok_or_else(writer_failed)?;
        let first_requested = self
            .pending
            .as_ref()
            .map_or(now, |pending| pending.first_requested);
        let urgent = urgent || self.pending.as_ref().is_some_and(|pending| pending.urgent);
        self.pending = Some(Pending {
            job: Job {
                revision: self.latest_revision,
                config,
                preferences,
            },
            first_requested,
            due: if urgent {
                now
            } else {
                (now + DEBOUNCE).min(first_requested + MAX_WAIT)
            },
            urgent,
        });
        self.dirty = true;
        Ok(())
    }
    pub fn flush(&mut self, now: Instant) {
        if let Some(pending) = &mut self.pending {
            pending.due = now;
            pending.urgent = true;
        }
    }
    pub fn busy(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }
    pub fn dirty(&self) -> bool {
        self.dirty
    }
    pub fn latest_revision(&self) -> u64 {
        self.latest_revision
    }

    /// Polling performs no file operations and never waits for a worker/channel.
    pub fn poll(&mut self, now: Instant) -> Option<Completion> {
        let mut completion = None;
        if let Some(revision) = self.active {
            let received = self
                .worker
                .as_ref()
                .expect("active writer")
                .results
                .try_recv();
            completion = match received {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Completion {
                    revision,
                    result: Err(writer_failed()),
                }),
            };
            if let Some(completed) = &completion {
                self.active = None;
                if completed.revision == self.latest_revision && completed.result.is_ok() {
                    self.dirty = false;
                }
            }
        }
        if self.active.is_none()
            && !self.closing
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| now >= pending.due)
        {
            // A failed worker can be replaced only after its exit is observed.
            if self.worker.as_ref().is_some_and(Worker::finished) {
                self.worker.take();
            }
            if self.worker.is_none() {
                match Worker::spawn(self.store.clone()) {
                    Ok(worker) => self.worker = Some(worker),
                    Err(error) => {
                        let pending = self.pending.take().expect("due");
                        return Some(Completion {
                            revision: pending.job.revision,
                            result: Err(error),
                        });
                    }
                }
            }
            let pending = self.pending.take().expect("due");
            let revision = pending.job.revision;
            match self
                .worker
                .as_ref()
                .expect("worker")
                .jobs
                .as_ref()
                .expect("open")
                .try_send(pending.job)
            {
                Ok(()) => self.active = Some(revision),
                Err(mpsc::TrySendError::Full(job)) => {
                    self.pending = Some(Pending { job, ..pending })
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Some(Completion {
                        revision,
                        result: Err(writer_failed()),
                    });
                }
            }
        }
        completion
    }
    pub fn close_if_idle(&mut self) {
        if !self.busy() {
            self.closing = true;
            if let Some(worker) = &mut self.worker {
                worker.jobs.take();
            }
        }
    }
    pub fn closed(&self) -> bool {
        self.closing && self.worker.as_ref().is_none_or(Worker::finished)
    }

    #[cfg(test)]
    pub(super) fn injected(
        store: impl Fn(&ControllerConfig, &Preferences) -> Result<(), AppError> + Send + Sync + 'static,
    ) -> Self {
        Self::with_store(Arc::new(store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn config() -> ControllerConfig {
        ControllerConfig {
            executable: "fixture".into(),
            token_file: None,
            settings_file: Some("unused-fixture-path".into()),
        }
    }
    fn preferences(value: u8) -> Preferences {
        Preferences {
            output: Some(crate::model::Output {
                volume_percent: value,
                volume_linear: None,
                muted: false,
            }),
            ..Preferences::default()
        }
    }
    fn settle(writer: &mut PreferencesWriter, now: Instant) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while writer.busy() {
            writer.poll(now);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn debounce_coalesces_bursts_but_sustained_changes_reach_the_deadline() {
        let saved = Arc::new(Mutex::new(Vec::new()));
        let observed = saved.clone();
        let mut writer = PreferencesWriter::injected(move |_, preferences| {
            observed
                .lock()
                .unwrap()
                .push(preferences.output.as_ref().unwrap().volume_percent);
            Ok(())
        });
        let start = Instant::now();
        for value in 0..10 {
            let now = start + Duration::from_millis(u64::from(value) * 50);
            writer
                .request(config(), preferences(value), now, false)
                .unwrap();
            writer.poll(now);
        }
        writer.poll(start + Duration::from_millis(499));
        assert!(writer.active.is_none());
        assert!(saved.lock().unwrap().is_empty());
        settle(&mut writer, start + Duration::from_millis(500));
        assert_eq!(*saved.lock().unwrap(), [9]);
        assert!(!writer.dirty());

        writer
            .request(
                config(),
                preferences(10),
                start + Duration::from_secs(1),
                false,
            )
            .unwrap();
        writer
            .request(
                config(),
                preferences(11),
                start + Duration::from_millis(1080),
                false,
            )
            .unwrap();
        writer.poll(start + Duration::from_millis(1229));
        assert!(writer.active.is_none());
        settle(&mut writer, start + Duration::from_millis(1230));
        assert_eq!(*saved.lock().unwrap(), [9, 11]);
    }

    #[test]
    fn one_blocked_write_keeps_only_the_latest_pending_setup() {
        let (started, starts) = mpsc::sync_channel(2);
        let (release, releases) = mpsc::sync_channel(2);
        let releases = Mutex::new(releases);
        let saved = Arc::new(Mutex::new(Vec::new()));
        let observed = saved.clone();
        let mut writer = PreferencesWriter::injected(move |_, preferences| {
            let value = preferences.output.as_ref().unwrap().volume_percent;
            started.send(value).unwrap();
            releases.lock().unwrap().recv().unwrap();
            observed.lock().unwrap().push(value);
            Ok(())
        });
        let now = Instant::now();
        writer.request(config(), preferences(1), now, true).unwrap();
        writer.poll(now);
        assert_eq!(starts.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        for value in 2..100 {
            writer
                .request(config(), preferences(value), now, false)
                .unwrap();
            writer.poll(now);
        }
        assert_eq!(
            writer
                .pending
                .as_ref()
                .unwrap()
                .job
                .preferences
                .output
                .as_ref()
                .unwrap()
                .volume_percent,
            99
        );
        assert!(starts.try_recv().is_err());
        writer.close_if_idle();
        assert!(!writer.closed());
        writer.flush(now);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while starts.try_recv().is_err() {
            writer.poll(now);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            writer.dirty(),
            "older success cannot settle newer pending state"
        );
        release.send(()).unwrap();
        settle(&mut writer, now);
        assert_eq!(*saved.lock().unwrap(), [1, 99]);
        writer.close_if_idle();
        while !writer.closed() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
