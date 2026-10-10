//! Included process-tree capture. IDs are valid for one process lifetime.
//! No executable matching, parent ascent, endpoint or exclusion fallback.

#[cfg(any(windows, test))]
use crate::model::{AppError, ErrorCode};

pub fn is_application_id(id: &str) -> bool {
    id.starts_with("app:")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(any(windows, test))]
struct Identity {
    pid: u32,
    created: u64,
}

#[cfg(any(windows, test))]
impl Identity {
    fn parse(id: &str) -> Result<Self, AppError> {
        let mut fields = id.split(':');
        let identity = match (fields.next(), fields.next(), fields.next(), fields.next()) {
            (Some("app"), Some(pid), Some(created), None) if created.len() == 16 => Self {
                pid: pid.parse().map_err(|_| invalid())?,
                created: u64::from_str_radix(created, 16).map_err(|_| invalid())?,
            },
            _ => return Err(invalid()),
        };
        if identity.pid == 0 || identity.created == 0 || identity.id() != id {
            return Err(invalid());
        }
        Ok(identity)
    }
    fn id(self) -> String {
        format!("app:{}:{:016x}", self.pid, self.created)
    }
}

#[cfg(any(windows, test))]
fn invalid() -> AppError {
    error("The selected application identity is invalid. Refresh and select a running application.")
}
#[cfg(any(windows, test))]
fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::CaptureFailed, message)
}

#[cfg(windows)]
pub use native::{
    enumerate_sources, identity_for_pid, roots_overlap, start_capture, validate_source,
};

#[cfg(windows)]
mod native {
    use super::*;
    use crate::audio::*;
    use crate::model::SourceKind;
    use ::windows::{
        Win32::{
            Media::Audio::*,
            System::{
                Com::{
                    BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                    StructuredStorage::{
                        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                    },
                },
                Variant::VT_BLOB,
            },
        },
        core::{HRESULT, IUnknown, Interface, Ref, implement},
    };
    use std::{
        collections::{BTreeSet, HashSet},
        mem::{ManuallyDrop, size_of},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr::{null, null_mut},
        sync::atomic::AtomicBool,
    };
    use windows_sys::Win32::{
        Foundation::{FILETIME, HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                CreateEventW, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
                WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GW_OWNER, GWL_EXSTYLE, GetDesktopWindow, GetShellWindow, GetWindow,
            GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
        },
    };

    const OPEN_DEADLINE: Duration = Duration::from_secs(3);
    const MAX_PACKET_FRAMES: usize = 4_800;

    fn handle(raw: HANDLE) -> Result<OwnedHandle, AppError> {
        if raw.is_null() || raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(error(
                "The selected application cannot be inspected. Select another running application.",
            ));
        }
        // SAFETY: every caller transfers one unique Win32 handle returned by a successful create/open.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }

    struct Target {
        handle: OwnedHandle,
        identity: Identity,
    }
    impl Target {
        fn open(pid: u32) -> Result<Self, AppError> {
            // SAFETY: query/synchronize only, non-inheritable, no access to process memory or termination.
            let handle = handle(unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            })?;
            let mut created = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            // SAFETY: handle and all four output structures remain live through the call.
            if unsafe {
                GetProcessTimes(
                    handle.as_raw_handle(),
                    &mut created,
                    &mut exit,
                    &mut kernel,
                    &mut user,
                )
            } == 0
            {
                return Err(error(
                    "The selected application identity cannot be verified.",
                ));
            }
            let identity = Identity {
                pid,
                created: (u64::from(created.dwHighDateTime) << 32)
                    | u64::from(created.dwLowDateTime),
            };
            let target = Self { handle, identity };
            if !target.alive() {
                return Err(exited());
            }
            Ok(target)
        }
        fn selected(id: &str) -> Result<Self, AppError> {
            let identity = Identity::parse(id)?;
            if shell_roots().contains(&identity.pid) || identity.pid == std::process::id() {
                return Err(error(
                    "Windows shell and Echocord processes cannot be selected for application capture.",
                ));
            }
            let target = Self::open(identity.pid)?;
            if target.identity != identity {
                return Err(exited());
            }
            Ok(target)
        }
        fn alive(&self) -> bool {
            // A held process handle remains the original process even if Windows later reuses its PID.
            unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) == WAIT_TIMEOUT }
        }
        fn basename(&self) -> Option<String> {
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            // SAFETY: bounded buffer, query-only live handle; full path is local and never leaves this function.
            if unsafe {
                QueryFullProcessImageNameW(
                    self.handle.as_raw_handle(),
                    PROCESS_NAME_WIN32,
                    buffer.as_mut_ptr(),
                    &mut length,
                )
            } == 0
            {
                return None;
            }
            String::from_utf16(&buffer[..length as usize])
                .ok()?
                .rsplit(['\\', '/'])
                .next()
                .map(str::to_owned)
        }
    }
    fn exited() -> AppError {
        error(
            "The selected application exited or restarted. Refresh sources and select its new instance.",
        )
    }
    pub fn identity_for_pid(pid: u32) -> Result<String, AppError> {
        Ok(Target::open(pid)?.identity.id())
    }
    pub fn validate_source(id: &str) -> Result<(), AppError> {
        Target::selected(id).map(|_| ())
    }

    pub fn roots_overlap(a: &str, b: &str) -> Result<bool, AppError> {
        let a = Identity::parse(a)?;
        let b = Identity::parse(b)?;
        if a.pid == b.pid {
            return Ok(true);
        }
        let snapshot = handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
            .map_err(|_| error("Application ancestry is unavailable. Refresh sources."))?;
        let mut parents = std::collections::HashMap::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut present = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } != 0;
        while present {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            present = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } != 0;
        }
        let contains = |mut pid: u32, root: u32| {
            let mut seen = HashSet::new();
            while seen.insert(pid) {
                if pid == root {
                    return true;
                }
                let Some(parent) = parents.get(&pid) else {
                    break;
                };
                pid = *parent;
            }
            false
        };
        Ok(contains(a.pid, b.pid) || contains(b.pid, a.pid))
    }

    fn own_tree() -> HashSet<u32> {
        let mut excluded = HashSet::from([std::process::id()]);
        // SAFETY: read-only process snapshot; entries are bounded to live OS process count.
        let Ok(snapshot) = handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
        else {
            return excluded;
        };
        let mut entries = Vec::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut present = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } != 0;
        while present {
            entries.push((entry.th32ProcessID, entry.th32ParentProcessID));
            present = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } != 0;
        }
        loop {
            let old = excluded.len();
            for &(pid, parent) in &entries {
                if excluded.contains(&parent) {
                    excluded.insert(pid);
                }
            }
            if excluded.len() == old {
                break;
            }
        }
        excluded
    }

    fn shell_roots() -> HashSet<u32> {
        let mut roots = HashSet::new();
        // Query OS window roles: shell trees can contain otherwise unrelated applications.
        for window in [unsafe { GetShellWindow() }, unsafe { GetDesktopWindow() }] {
            if !window.is_null() {
                let mut pid = 0;
                unsafe { GetWindowThreadProcessId(window, &mut pid) };
                if pid != 0 {
                    roots.insert(pid);
                }
            }
        }
        roots
    }

    pub fn enumerate_sources() -> Result<Vec<RecordingSource>, AppError> {
        unsafe extern "system" fn collect(window: HWND, context: isize) -> i32 {
            // SAFETY: EnumWindows invokes this synchronously; context points to the caller's live set.
            let roots = unsafe { &mut *(context as *mut BTreeSet<u32>) };
            if unsafe { IsWindowVisible(window) } != 0
                && unsafe { GetWindow(window, GW_OWNER) }.is_null()
                && unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } & WS_EX_TOOLWINDOW as isize
                    == 0
            {
                let mut pid = 0;
                unsafe { GetWindowThreadProcessId(window, &mut pid) };
                if pid != 0 {
                    roots.insert(pid);
                }
            }
            1
        }
        let mut excluded = own_tree();
        excluded.extend(shell_roots());
        let mut roots = BTreeSet::new();
        // SAFETY: callback does no async work and cannot outlive the borrowed local context.
        if unsafe { EnumWindows(Some(collect), (&mut roots as *mut BTreeSet<u32>) as isize) } == 0 {
            return Err(error(
                "Windows application discovery failed. Refresh sources and retry.",
            ));
        }
        let mut sources = Vec::new();
        for pid in roots {
            if excluded.contains(&pid) {
                continue;
            }
            let Ok(target) = Target::open(pid) else {
                continue;
            };
            let Some(name) = target.basename() else {
                continue;
            };
            if !target.alive() {
                continue;
            }
            sources.push(RecordingSource {
                id: target.identity.id(),
                name,
                kind: SourceKind::Application,
                available: true,
                detail: Some(format!("Process {pid} and its child processes")),
            });
        }
        sources.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        Ok(sources)
    }

    struct Com;
    impl Com {
        fn enter() -> Result<Self, AppError> {
            // Fresh owned thread, balanced exactly once including S_FALSE success.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|_| error("Windows application capture could not initialize."))?;
            Ok(Self)
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// Stable, immutable input storage; the variant points only to the owned parameter allocation.
    /// No COM interface or mutable aliased data lives here. Windows only reads it, and its callback
    /// holds this allocation until completion even if our opening thread cancels or times out.
    struct RequestStorage {
        _params: Box<AUDIOCLIENT_ACTIVATION_PARAMS>,
        variant: ManuallyDrop<PROPVARIANT>,
    }
    // SAFETY: the sole embedded pointer targets the immutable owned Box above. No pointer is exposed
    // for mutation; relocation moves the Box owner, never the allocation. Drop never invokes VariantClear.
    unsafe impl Send for RequestStorage {}
    unsafe impl Sync for RequestStorage {}
    impl RequestStorage {
        fn new(pid: u32) -> Self {
            let mut params = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
                ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
                Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                    ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                        TargetProcessId: pid,
                        ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                    },
                },
            });
            let variant = ManuallyDrop::new(PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_BLOB,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 {
                            blob: BLOB {
                                cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                                pBlobData: (&mut *params as *mut AUDIOCLIENT_ACTIVATION_PARAMS)
                                    .cast(),
                            },
                        },
                    }),
                },
            });
            Self {
                _params: params,
                variant,
            }
        }
    }
    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Completion {
        _request: Arc<RequestStorage>,
        complete: Arc<AtomicBool>,
    }
    impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
        fn ActivateCompleted(
            &self,
            _: Ref<IActivateAudioInterfaceAsyncOperation>,
        ) -> ::windows::core::Result<()> {
            self.complete.store(true, Ordering::Release);
            Ok(())
        }
    }

    fn activate(target: &Target, shared: &Shared) -> Result<IAudioClient, AppError> {
        let complete = Arc::new(AtomicBool::new(false));
        let request = Arc::new(RequestStorage::new(target.identity.pid));
        // windows-rs implement macro provides IAgileObject/IMarshal by default. Completion only signals;
        // it never gets a client or starts audio, so late completion cannot revive abandoned capture.
        let callback: IActivateAudioInterfaceCompletionHandler = Completion {
            _request: request.clone(),
            complete: complete.clone(),
        }
        .into();
        let operation = unsafe { ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID,
            Some(&*request.variant), &callback) }.map_err(|_| error("Application capture is unsupported or access was denied. Use a recording input or another application."))?;
        wait_activation(&complete, shared, || target.alive())?;
        if !target.alive() || shared.status.load(Ordering::Acquire) != OPENING {
            return Err(exited());
        }
        let mut result = HRESULT(0);
        let mut interface: Option<IUnknown> = None;
        unsafe { operation.GetActivateResult(&mut result, &mut interface) }
            .map_err(|_| error("Application capture activation failed."))?;
        result.ok().map_err(|_| error("Application capture is unsupported or access was denied. Use a recording input or another application."))?;
        interface
            .ok_or_else(|| error("Application capture returned no audio interface."))?
            .cast()
            .map_err(|_| error("Application capture returned an unsupported audio interface."))
    }

    fn wait_activation(
        complete: &AtomicBool,
        shared: &Shared,
        alive: impl Fn() -> bool,
    ) -> Result<(), AppError> {
        let deadline = Instant::now() + OPEN_DEADLINE;
        while !complete.load(Ordering::Acquire) {
            if shared.status.load(Ordering::Acquire) != OPENING {
                return Err(error("Application capture was cancelled."));
            }
            if !alive() {
                return Err(exited());
            }
            if Instant::now() >= deadline {
                return Err(error(
                    "Application capture activation timed out. Reconnect and retry.",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn stopped_opening(session: &CaptureSession, cause: AppError) -> AppError {
        if session.control.stop() {
            cause
        } else {
            AppError::new(
                ErrorCode::CleanupIncomplete,
                "Native capture cleanup did not finish. Restart Echocord.",
            )
        }
    }

    pub fn start_capture(
        id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CaptureSession, AppError> {
        let target = Target::selected(id)?;
        let (writer, session) = pipeline(Arc::new(SystemClock(Instant::now())));
        let shared = writer.shared.clone();
        let (ready, result) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("application-capture".into())
            .spawn(move || {
                let outcome = pump(target, writer, &ready);
                if let Err(error) = outcome {
                    shared.fail(FAILED);
                    let _ = ready.try_send(Err(error));
                }
            })
            .map_err(|_| error("Application capture thread could not start."))?;
        *session
            .control
            .owner
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(worker);
        let deadline = Instant::now() + OPEN_DEADLINE + Duration::from_millis(500);
        loop {
            if cancelled() {
                return Err(stopped_opening(
                    &session,
                    error("Application capture was cancelled."),
                ));
            }
            match result.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(())) if !cancelled() => return Ok(session),
                Ok(Ok(())) => {
                    return Err(stopped_opening(
                        &session,
                        error("Application capture was cancelled."),
                    ));
                }
                Ok(Err(error)) => {
                    return Err(stopped_opening(&session, error));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(stopped_opening(
                        &session,
                        error("Application capture stopped while opening."),
                    ));
                }
                Err(_) if Instant::now() >= deadline => {
                    return Err(stopped_opening(
                        &session,
                        error("Application capture opening timed out. Reconnect and retry."),
                    ));
                }
                Err(_) => {}
            }
        }
    }

    fn pump(
        target: Target,
        mut writer: CaptureWriter,
        ready: &std::sync::mpsc::SyncSender<Result<(), AppError>>,
    ) -> Result<(), AppError> {
        let _com = Com::enter()?;
        let event = handle(unsafe { CreateEventW(null(), 0, 0, null()) })?;
        let client = activate(&target, &writer.shared)?;
        let format = WAVEFORMATEX {
            wFormatTag: 3,
            nChannels: 2,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                0,
                0,
                &format,
                None,
            )
        }
        .map_err(|_| error("Application capture could not open 48 kHz stereo PCM."))?;
        unsafe {
            client.SetEventHandle(::windows::Win32::Foundation::HANDLE(event.as_raw_handle()))
        }
        .map_err(|_| error("Application capture notification setup failed."))?;
        let capture: IAudioCaptureClient = unsafe { client.GetService() }
            .map_err(|_| error("Application capture packet setup failed."))?;
        if !target.alive() {
            writer.shared.fail(APPLICATION_EXITED);
            return Err(exited());
        }
        if writer.shared.status.load(Ordering::Acquire) != OPENING {
            return Ok(());
        }
        unsafe { client.Start() }.map_err(|_| error("Application capture could not start."))?;
        writer
            .shared
            .status
            .compare_exchange(OPENING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .ok();
        if writer.shared.status.load(Ordering::Acquire) != RUNNING {
            unsafe { client.Stop() }.ok();
            return Ok(());
        }
        let _ = ready.try_send(Ok(()));
        let mut scratch = [0.0_f32; MAX_PACKET_FRAMES * 2];
        let result = (|| {
            while writer.shared.status.load(Ordering::Acquire) == RUNNING {
                if !target.alive() {
                    writer.shared.fail(APPLICATION_EXITED);
                    return Err(exited());
                }
                // Never drain indefinitely under a producer race; check intent and identity between packets.
                for _ in 0..16 {
                    if writer.shared.status.load(Ordering::Acquire) != RUNNING {
                        break;
                    }
                    if !target.alive() {
                        writer.shared.fail(APPLICATION_EXITED);
                        return Err(exited());
                    }
                    if unsafe { capture.GetNextPacketSize() }
                        .map_err(|_| error("Application capture packets are unavailable."))?
                        == 0
                    {
                        break;
                    }
                    read_packet(&capture, &mut writer, &mut scratch)?;
                }
                let wait = unsafe { WaitForSingleObject(event.as_raw_handle(), 20) };
                if wait != WAIT_OBJECT_0 && wait != WAIT_TIMEOUT {
                    return Err(error("Application capture notification failed."));
                }
            }
            Ok(())
        })();
        unsafe { client.Stop() }.map_err(|_| error("Application capture did not stop cleanly."))?;
        result
    }

    fn read_packet(
        capture: &IAudioCaptureClient,
        writer: &mut CaptureWriter,
        scratch: &mut [f32; MAX_PACKET_FRAMES * 2],
    ) -> Result<(), AppError> {
        let mut data = null_mut();
        let mut frames = 0;
        let mut flags = 0;
        unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
            .map_err(|_| error("Application capture packet read failed."))?;
        if frames == 0 {
            return Ok(());
        }
        struct Packet<'a>(&'a IAudioCaptureClient, u32);
        impl Drop for Packet<'_> {
            fn drop(&mut self) {
                unsafe { self.0.ReleaseBuffer(self.1) }.ok();
            }
        }
        let packet = Packet(capture, frames);
        if frames as usize > MAX_PACKET_FRAMES {
            return Err(error(
                "Application capture exceeded the bounded packet size.",
            ));
        }
        if flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 {
            writer.shared.record_xrun();
            writer.shared.input_epoch.fetch_add(1, Ordering::AcqRel);
            writer.partial_left = None;
        }
        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
        if !silent && data.is_null() {
            return Err(error("Application capture returned an invalid packet."));
        }
        // Do not inspect QPC/device timestamps: queue age measures native exposure to reader only.
        // For SILENT, the pointer may be null and is never dereferenced.
        let samples = &mut scratch[..frames as usize * 2];
        if silent {
            samples.fill(0.0);
        } else {
            for (index, sample) in samples.iter_mut().enumerate() {
                // SAFETY: non-null packet has frames*8 initialized bytes until ReleaseBuffer.
                *sample = unsafe { data.add(index * 4).cast::<f32>().read_unaligned() };
            }
        }
        writer.write(samples, 2, |sample| sample);
        // Observe release errors in the normal path; the guard still handles every early return.
        let frames = packet.1;
        std::mem::forget(packet);
        unsafe { capture.ReleaseBuffer(frames) }
            .map_err(|_| error("Application capture packet release failed."))
    }

    #[cfg(test)]
    mod lifecycle_tests {
        use super::*;
        #[test]
        fn pending_activation_wait_observes_cancellation_before_completion() {
            let (writer, _session) = pipeline(Arc::new(SystemClock(Instant::now())));
            let shared = writer.shared.clone();
            let cancel = shared.clone();
            let complete = AtomicBool::new(false);
            let worker = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                cancel.status.store(STOPPED, Ordering::Release);
            });
            let started = Instant::now();
            let failure = wait_activation(&complete, &shared, || true).unwrap_err();
            worker.join().unwrap();
            assert!(failure.message.contains("cancelled"));
            assert!(!complete.load(Ordering::Acquire));
            assert!(started.elapsed() < Duration::from_millis(100));
        }

        #[test]
        fn late_completion_owns_activation_storage_and_only_signals() {
            let request = Arc::new(RequestStorage::new(42));
            let lifetime = Arc::downgrade(&request);
            let complete = Arc::new(AtomicBool::new(false));
            let callback: IActivateAudioInterfaceCompletionHandler = Completion {
                _request: request.clone(),
                complete: complete.clone(),
            }
            .into();
            assert!(
                callback
                    .cast::<::windows::Win32::System::Com::IAgileObject>()
                    .is_ok()
            );
            drop(request);
            assert!(lifetime.upgrade().is_some());
            unsafe { callback.ActivateCompleted(None::<&IActivateAudioInterfaceAsyncOperation>) }
                .unwrap();
            assert!(complete.load(Ordering::Acquire));
            drop(callback);
            assert!(lifetime.upgrade().is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_requires_canonical_pid_and_creation_time() {
        let identity = Identity {
            pid: 42,
            created: 0x123456789abc,
        };
        assert_eq!(Identity::parse(&identity.id()).unwrap(), identity);
        for id in [
            "app:42",
            "app:0:0000000000000001",
            "app:42:0000000000000000",
            "app:042:0000000000000001",
            "app:42:000000000000000F",
            "app:42:1",
            "app:42:0000000000000001:extra",
        ] {
            assert!(Identity::parse(id).is_err(), "{id}");
        }
    }
}
