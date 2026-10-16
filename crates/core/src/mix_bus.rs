//! Bounded native-only stereo transport. The desktop parent owns a mapping handle
//! but never maps it. Every shared access is aligned atomic, including samples.
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

pub const CAPACITY: usize = 4096;
const LEGACY_CAPACITY: usize = 1920;
#[cfg(any(windows, test))]
const CORRECTION_MAX_AGE_MS: u64 = 80;
const MAX_AGE_MS: u64 = 40;
const VERSION: u64 = 3;
const MAGIC: u64 = 0x4543484f4d495831;
#[repr(C, align(8))]
struct Slot {
    sequence: AtomicU64,
    stereo: AtomicU64,
    exposed_ms: AtomicU64,
}
#[repr(C, align(8))]
pub struct Bus {
    magic: AtomicU64,
    version: AtomicU64,
    capacity: AtomicU64,
    cursor: AtomicU64,
    heartbeat_ms: AtomicU64,
    continuity_floor: AtomicU64,
    slots: [Slot; CAPACITY],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadStats {
    pub queued_frames: u32,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
}

/// Snapshot for bounded admission; a subsequent exact pull validates it again.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CorrectionWindow {
    pub latest: u64,
    pub available: usize,
    pub continuity: u64,
    pub alive: bool,
}
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CorrectionRead {
    pub valid_frames: usize,
    pub dropped_frames: u32,
    pub queued_frames: u32,
    pub oldest_ms: Option<u64>,
    pub continuity: u64,
}

impl Bus {
    #[cfg(test)]
    pub(crate) fn test_bus() -> Box<Self> {
        let bus = Box::new(Self {
            magic: AtomicU64::new(0),
            version: AtomicU64::new(0),
            capacity: AtomicU64::new(0),
            cursor: AtomicU64::new(0),
            heartbeat_ms: AtomicU64::new(0),
            continuity_floor: AtomicU64::new(0),
            slots: std::array::from_fn(|_| Slot {
                sequence: AtomicU64::new(0),
                stereo: AtomicU64::new(0),
                exposed_ms: AtomicU64::new(0),
            }),
        });
        bus.initialize();
        bus
    }
    pub fn initialize(&self) {
        self.cursor.store(0, SeqCst);
        self.heartbeat_ms.store(0, SeqCst);
        self.continuity_floor.store(0, SeqCst);
        self.version.store(VERSION, SeqCst);
        self.capacity.store(CAPACITY as u64, SeqCst);
        self.magic.store(MAGIC, SeqCst);
    }
    pub fn valid(&self) -> bool {
        self.magic.load(SeqCst) == MAGIC
            && self.version.load(SeqCst) == VERSION
            && self.capacity.load(SeqCst) == CAPACITY as u64
    }
    pub fn cursor(&self) -> u64 {
        self.cursor.load(SeqCst)
    }
    pub fn write(&self, frames: &[[f32; 2]], now_ms: u64) {
        let mut cursor = self.cursor();
        if now_ms.saturating_sub(self.heartbeat_ms.load(SeqCst)) > MAX_AGE_MS {
            // Publish this fence before renewing the heartbeat. A reader that
            // observes the renewed producer must exclude every older frame,
            // including when both processes were suspended through the gap.
            self.continuity_floor.store(cursor, SeqCst);
        }
        for frame in frames {
            let slot = &self.slots[cursor as usize % CAPACITY];
            slot.sequence
                .store(cursor.wrapping_mul(2).wrapping_add(1), SeqCst);
            slot.stereo.store(
                u64::from(frame[0].to_bits()) | (u64::from(frame[1].to_bits()) << 32),
                SeqCst,
            );
            slot.exposed_ms.store(now_ms, SeqCst);
            slot.sequence
                .store(cursor.wrapping_mul(2).wrapping_add(2), SeqCst);
            cursor += 1;
        }
        self.heartbeat_ms.store(now_ms, SeqCst);
        self.cursor.store(cursor, SeqCst);
    }
    #[cfg(any(windows, test))]
    pub(crate) fn correction_window(&self, cursor: u64, now_ms: u64) -> CorrectionWindow {
        let latest = self.cursor();
        let heartbeat = self.heartbeat_ms.load(SeqCst);
        let continuity = self.continuity_floor.load(SeqCst);
        let alive = self.valid() && now_ms.saturating_sub(heartbeat) <= MAX_AGE_MS;
        let oldest = latest
            .saturating_sub(CAPACITY as u64)
            .max(continuity.min(latest));
        CorrectionWindow {
            latest,
            available: if alive {
                latest
                    .saturating_sub(cursor.max(oldest))
                    .min(CAPACITY as u64) as usize
            } else {
                0
            },
            continuity,
            alive,
        }
    }
    #[cfg(any(windows, test))]
    pub(crate) fn correction_lease_valid(
        &self,
        continuity: u64,
        oldest_ms: u64,
        now_ms: u64,
    ) -> bool {
        self.valid()
            && self.continuity_floor.load(SeqCst) == continuity
            && now_ms.saturating_sub(self.heartbeat_ms.load(SeqCst)) <= MAX_AGE_MS
            && oldest_ms <= now_ms.saturating_add(1)
            && now_ms.saturating_sub(oldest_ms) <= CORRECTION_MAX_AGE_MS
    }
    /// No synthetic silence enters a correction filter. Only an entirely
    /// coherent/fresh block returns a nonzero valid_frames count.
    #[cfg(test)]
    pub(crate) fn read_correction(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        now_ms: u64,
    ) -> CorrectionRead {
        self.read_correction_clocked(
            cursor,
            frames,
            None,
            now_ms,
            completion_clock(now_ms),
            |_| {},
        )
    }
    #[cfg(any(windows, test))]
    pub(crate) fn read_correction_with_origins(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        origins_ns: &mut [u64],
        now_ms: u64,
    ) -> CorrectionRead {
        if origins_ns.len() != frames.len() {
            frames.fill([0.0; 2]);
            return CorrectionRead::default();
        }
        self.read_correction_clocked(
            cursor,
            frames,
            Some(origins_ns),
            now_ms,
            completion_clock(now_ms),
            |_| {},
        )
    }
    #[cfg(any(windows, test))]
    fn read_correction_clocked(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        mut origins_ns: Option<&mut [u64]>,
        now_ms: u64,
        completed: impl FnOnce() -> u64,
        mut observe: impl FnMut(ReadBoundary),
    ) -> CorrectionRead {
        frames.fill([0.0; 2]);
        let original = *cursor;
        let window = self.correction_window(*cursor, now_ms);
        observe(ReadBoundary::Floor);
        let mut result = CorrectionRead {
            continuity: window.continuity,
            ..CorrectionRead::default()
        };
        if !window.alive || frames.len() > CAPACITY {
            *cursor = window.latest;
            result.dropped_frames = cursor.saturating_sub(original).min(u32::MAX as u64) as u32;
            return result;
        }
        let oldest = window
            .latest
            .saturating_sub(CAPACITY as u64)
            .max(window.continuity.min(window.latest));
        *cursor = (*cursor).max(oldest);
        result.dropped_frames = cursor.saturating_sub(original).min(u32::MAX as u64) as u32;
        let available = window.latest.saturating_sub(*cursor) as usize;
        if frames.len() > available {
            result.queued_frames = available.min(CAPACITY) as u32;
            return result;
        }
        let mut valid = true;
        let mut oldest_ms = u64::MAX;
        for (index, frame) in frames.iter_mut().enumerate() {
            let slot = &self.slots[*cursor as usize % CAPACITY];
            let expected = cursor.wrapping_mul(2).wrapping_add(2);
            let first = slot.sequence.load(SeqCst);
            let stereo = slot.stereo.load(SeqCst);
            let exposure = slot.exposed_ms.load(SeqCst);
            let last = slot.sequence.load(SeqCst);
            let left = f32::from_bits(stereo as u32);
            let right = f32::from_bits((stereo >> 32) as u32);
            if first != expected
                || last != expected
                || !left.is_finite()
                || !right.is_finite()
                || exposure > now_ms.saturating_add(1)
                || now_ms.saturating_sub(exposure) > CORRECTION_MAX_AGE_MS
            {
                valid = false;
            } else {
                *frame = [left, right];
                oldest_ms = oldest_ms.min(exposure);
                if let Some(origins) = origins_ns.as_deref_mut() {
                    origins[index] = exposure.saturating_mul(1_000_000);
                }
            }
            *cursor += 1;
        }
        observe(ReadBoundary::Copied);
        let final_ms = completed();
        if !valid
            || final_ms.saturating_sub(now_ms) > MAX_AGE_MS
            || !self.correction_lease_valid(window.continuity, oldest_ms, final_ms)
        {
            frames.fill([0.0; 2]);
            *cursor = (*cursor).max(self.continuity_floor.load(SeqCst));
            if final_ms.saturating_sub(now_ms) > MAX_AGE_MS {
                *cursor = (*cursor).max(window.latest);
            }
            result.dropped_frames = cursor.saturating_sub(original).min(u32::MAX as u64) as u32;
        } else if !frames.is_empty() {
            result.valid_frames = frames.len();
            result.oldest_ms = Some(oldest_ms);
        }
        result.queued_frames = self.cursor().saturating_sub(*cursor).min(CAPACITY as u64) as u32;
        result
    }
    pub fn read(&self, cursor: &mut u64, frames: &mut [[f32; 2]], now_ms: u64) {
        let _ = self.read_counted(cursor, frames, now_ms);
    }
    pub fn read_counted(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        now_ms: u64,
    ) -> ReadStats {
        self.read_clocked(cursor, frames, now_ms, completion_clock(now_ms), |_| {})
    }

    #[cfg(test)]
    fn read_observed(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        now_ms: u64,
        observe: impl FnMut(ReadBoundary),
    ) -> ReadStats {
        self.read_clocked(cursor, frames, now_ms, || now_ms, observe)
    }

    fn read_clocked(
        &self,
        cursor: &mut u64,
        frames: &mut [[f32; 2]],
        now_ms: u64,
        completed_ms: impl FnOnce() -> u64,
        mut observe: impl FnMut(ReadBoundary),
    ) -> ReadStats {
        frames.fill([0.0; 2]);
        let original_cursor = *cursor;
        let latest = self.cursor();
        observe(ReadBoundary::Cursor);
        let heartbeat = self.heartbeat_ms.load(SeqCst);
        observe(ReadBoundary::Heartbeat);
        let mut stats = ReadStats::default();
        if !self.valid() || now_ms.saturating_sub(heartbeat) > MAX_AGE_MS {
            stats.dropped_frames = latest.saturating_sub(*cursor).min(u32::MAX as u64) as u32;
            stats.underrun_frames = frames.len().min(u32::MAX as usize) as u32;
            *cursor = latest;
            return stats;
        }
        // SeqCst: renewed heartbeat -> preceding floor store -> this load.
        // A newer floor than our cursor snapshot conservatively yields silence.
        let floor = self.continuity_floor.load(SeqCst);
        observe(ReadBoundary::Floor);
        let oldest = latest
            .saturating_sub(LEGACY_CAPACITY as u64)
            .max(floor.min(latest));
        stats.dropped_frames = oldest.saturating_sub(*cursor).min(u32::MAX as u64) as u32;
        *cursor = (*cursor).max(oldest);
        for frame in frames.iter_mut() {
            if *cursor >= latest {
                stats.underrun_frames = stats.underrun_frames.saturating_add(1);
                continue;
            }
            let slot = &self.slots[*cursor as usize % CAPACITY];
            let expected = cursor.wrapping_mul(2).wrapping_add(2);
            let first = slot.sequence.load(SeqCst);
            let stereo = slot.stereo.load(SeqCst);
            let last = slot.sequence.load(SeqCst);
            if first == expected && last == expected {
                let left = f32::from_bits(stereo as u32);
                let right = f32::from_bits((stereo >> 32) as u32);
                if left.is_finite() && right.is_finite() {
                    *frame = [left, right];
                }
            } else {
                stats.dropped_frames = stats.dropped_frames.saturating_add(1);
            }
            *cursor += 1;
        }
        observe(ReadBoundary::Copied);
        let final_floor = self.continuity_floor.load(SeqCst);
        let completed_ms = completed_ms();
        let expired = completed_ms.saturating_sub(now_ms) > MAX_AGE_MS
            || completed_ms.saturating_sub(heartbeat) > MAX_AGE_MS;
        if final_floor != floor || expired {
            // A reader can itself be suspended while copying. Do not return a
            // block assembled across a producer discontinuity or a long reader
            // pause, even if the producer has not resumed to publish a floor.
            // This final clock check cannot guard a pause after it, including
            // between returning the buffer and the device accepting it.
            frames.fill([0.0; 2]);
            *cursor = (*cursor).max(final_floor);
            if expired {
                // Discard only data observed at entry; a fresh producer suffix
                // remains available to the next read. Never invent cursor gaps.
                *cursor = (*cursor).max(latest);
            }
            return ReadStats {
                dropped_frames: (*cursor)
                    .saturating_sub(original_cursor)
                    .min(u32::MAX as u64) as u32,
                underrun_frames: frames.len().min(u32::MAX as usize) as u32,
                queued_frames: self
                    .cursor()
                    .saturating_sub(*cursor)
                    .min(LEGACY_CAPACITY as u64) as u32,
            };
        }
        stats.queued_frames = latest.saturating_sub(*cursor).min(LEGACY_CAPACITY as u64) as u32;
        stats
    }
}

fn completion_clock(started_ms: u64) -> impl FnOnce() -> u64 {
    move || {
        #[cfg(all(windows, not(test)))]
        {
            let _ = started_ms;
            native::now_ms()
        }
        // Portable/native unit fixtures supply a virtual timestamp. They inject
        // an advancing completion clock directly when testing read suspension.
        #[cfg(any(not(windows), test))]
        {
            started_ms
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadBoundary {
    Cursor,
    Heartbeat,
    Floor,
    Copied,
}

#[cfg(any(windows, test))]
fn counter_millis(counter: u64, frequency: u64) -> u64 {
    (u128::from(counter) * 1000 / u128::from(frequency)).min(u128::from(u64::MAX)) as u64
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        io,
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr::null,
    };
    use windows_sys::Win32::{
        Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, INVALID_HANDLE_VALUE},
        System::{
            Memory::{
                CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
                PAGE_READWRITE, UnmapViewOfFile,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Threading::GetCurrentProcess,
        },
    };
    pub struct BusHandle(pub OwnedHandle);
    impl BusHandle {
        pub fn create() -> io::Result<Self> {
            let raw = unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    null(),
                    PAGE_READWRITE,
                    0,
                    size_of::<Bus>() as u32,
                    null(),
                )
            };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(unsafe { OwnedHandle::from_raw_handle(raw) }))
        }
        pub fn inheritable(&self) -> io::Result<OwnedHandle> {
            let mut copy = std::ptr::null_mut();
            if unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    self.0.as_raw_handle(),
                    GetCurrentProcess(),
                    &mut copy,
                    0,
                    1,
                    DUPLICATE_SAME_ACCESS,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedHandle::from_raw_handle(copy) })
        }
    }
    pub struct Mapping {
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        _handle: OwnedHandle,
    }
    // The view is a fixed, page-aligned Bus containing only atomics, with one writer.
    unsafe impl Send for Mapping {}
    impl Mapping {
        pub fn inherited(raw: usize) -> io::Result<Self> {
            // Warm the immutable clock frequency before any audio callback.
            let _ = clock_frequency();
            let handle = unsafe { OwnedHandle::from_raw_handle(raw as _) };
            let view = unsafe {
                MapViewOfFile(
                    handle.as_raw_handle(),
                    FILE_MAP_ALL_ACCESS,
                    0,
                    0,
                    size_of::<Bus>(),
                )
            };
            if view.Value.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                view,
                _handle: handle,
            })
        }
        pub fn bus(&self) -> &Bus {
            unsafe { &*self.view.Value.cast::<Bus>() }
        }
    }
    impl Drop for Mapping {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.view);
            }
        }
    }
    fn clock_frequency() -> u64 {
        static FREQUENCY: AtomicU64 = AtomicU64::new(0);
        let cached = FREQUENCY.load(SeqCst);
        if cached != 0 {
            return cached;
        }
        let mut frequency = 0;
        // Windows 11 guarantees QPC support and a fixed boot-wide frequency.
        assert!(unsafe { QueryPerformanceFrequency(&mut frequency) } != 0 && frequency > 0);
        FREQUENCY.store(frequency as u64, SeqCst);
        frequency as u64
    }
    pub fn now_ms() -> u64 {
        let frequency = clock_frequency();
        let mut counter = 0;
        assert!(unsafe { QueryPerformanceCounter(&mut counter) } != 0 && counter >= 0);
        // QPC has a common origin across processes. GetTickCount64's 10–16 ms
        // quantization is unsuitable for the bus's 40 ms freshness boundary.
        counter_millis(counter as u64, frequency)
    }
    #[cfg(test)]
    #[test]
    fn shared_milliseconds_are_bracketed_by_native_counter_samples() {
        let frequency = clock_frequency();
        let mut before = 0;
        let mut after = 0;
        assert_ne!(unsafe { QueryPerformanceCounter(&mut before) }, 0);
        let milliseconds = now_ms();
        assert_ne!(unsafe { QueryPerformanceCounter(&mut after) }, 0);
        assert!(milliseconds >= counter_millis(before as u64, frequency));
        assert!(milliseconds <= counter_millis(after as u64, frequency));
        assert_eq!(clock_frequency(), frequency);
    }
    #[cfg(test)]
    #[test]
    fn native_completion_clock_rejects_a_paused_read_without_opening_devices() {
        let bus = Bus::test_bus();
        let started = now_ms();
        bus.write(&[[0.75, -0.25]; 240], started);
        let mut cursor = 0;
        let mut frames = [[0.0; 2]; 240];
        let stats = bus.read_clocked(&mut cursor, &mut frames, started, now_ms, |point| {
            if point == ReadBoundary::Copied {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        assert_eq!(frames, [[0.0; 2]; 240]);
        assert_eq!(cursor, 240);
        assert_eq!(stats.dropped_frames, 240);
        assert_eq!(stats.underrun_frames, 240);
    }
    pub fn capture(id: &str) -> Result<crate::audio::CaptureSession, crate::model::AppError> {
        let failure = || {
            crate::model::AppError::new(
                crate::model::ErrorCode::CaptureFailed,
                "The shared audio mix is unavailable.",
            )
        };
        let raw = id
            .strip_prefix("mix-handle:")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n != 0)
            .ok_or_else(failure)?;
        let mapping = Mapping::inherited(raw).map_err(|_| failure())?;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !mapping.bus().valid() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if !mapping.bus().valid() {
            return Err(failure());
        }
        // Every new network worker attaches at the present end, never old audio.
        let mut cursor = mapping.bus().cursor();
        Ok(crate::audio::bus_capture(move |frames| {
            mapping.bus().read(&mut cursor, frames, now_ms())
        }))
    }
}
#[cfg(windows)]
pub use native::*;

#[cfg(test)]
mod tests {
    use super::*;
    // Existing read/voice fixtures deliberately retain the old logical window.
    const CAPACITY: usize = LEGACY_CAPACITY;
    fn bus() -> Box<Bus> {
        Bus::test_bus()
    }
    #[test]
    fn correction_reads_distinguish_valid_silence_from_an_incomplete_block() {
        let bus = bus();
        bus.write(&[[0.0; 2]; 300], 100);
        let window = bus.correction_window(0, 100);
        assert!(window.alive);
        assert_eq!(window.available, 300);
        let mut cursor = 0;
        let mut frames = [[9.0; 2]; 400];
        let short = bus.read_correction(&mut cursor, &mut frames, 100);
        assert_eq!(short.valid_frames, 0);
        assert_eq!(cursor, 0);
        assert_eq!(short.queued_frames, 300);
        assert!(frames.iter().all(|frame| *frame == [0.0; 2]));
        let silence = bus.read_correction(&mut cursor, &mut frames[..300], 100);
        assert_eq!(silence.valid_frames, 300);
        assert_eq!(silence.oldest_ms, Some(100));
        assert_eq!(silence.continuity, window.continuity);
        assert_eq!(cursor, 300);
    }

    #[test]
    fn correction_capacity_does_not_expand_legacy_or_voice_retention() {
        let bus = bus();
        let source: Vec<_> = (1..=3000).map(|n| [n as f32, -(n as f32)]).collect();
        bus.write(&source, 10);
        assert_eq!(bus.correction_window(0, 10).available, 3000);
        let mut corrected = 0;
        let mut legacy = 0;
        let mut output = [[0.0; 2]; 240];
        let result = bus.read_correction(&mut corrected, &mut output, 10);
        assert_eq!(result.valid_frames, 240);
        assert_eq!(output[0], [1.0, -1.0]);
        let result = bus.read_counted(&mut legacy, &mut output, 10);
        assert_eq!(result.dropped_frames, 1080);
        assert_eq!(output[0], [1081.0, -1081.0]);
        assert!(result.queued_frames <= LEGACY_CAPACITY as u32);
    }

    #[test]
    fn correction_sample_age_is_independent_of_recent_producer_activity() {
        for age in [80, 81] {
            let bus = bus();
            bus.write(&[[0.5, -0.25]; 240], 100);
            for tick in 1..=4 {
                bus.write(&[[0.125, -0.0625]; 240], 100 + tick * 20);
            }
            let mut cursor = 0;
            let mut output = [[0.0; 2]; 240];
            let result = bus.read_correction(&mut cursor, &mut output, 100 + age);
            if age == 80 {
                assert_eq!(result.valid_frames, 240);
                assert_eq!(result.oldest_ms, Some(100));
            } else {
                assert_eq!(result.valid_frames, 0);
                assert_eq!(result.dropped_frames, 240);
                assert_eq!(output, [[0.0; 2]; 240]);
            }
        }
    }

    #[test]
    fn correction_completion_checks_reader_pause_and_changed_continuity() {
        for producer_resumed in [false, true] {
            let bus = bus();
            bus.write(&[[0.75, -0.25]; 960], 0);
            let mut cursor = 0;
            let mut output = [[0.0; 2]; 240];
            let result = bus.read_correction_clocked(
                &mut cursor,
                &mut output,
                None,
                1,
                || 1000,
                |boundary| {
                    if producer_resumed && boundary == ReadBoundary::Copied {
                        bus.write(&[[0.125, -0.0625]; 240], 1000);
                    }
                },
            );
            assert_eq!(result.valid_frames, 0);
            assert_eq!(output, [[0.0; 2]; 240]);
            assert_eq!(cursor, 960);
            assert_eq!(result.dropped_frames, 960);
            if !producer_resumed {
                bus.write(&[[0.125, -0.0625]; 240], 1000);
            }
            let fresh = bus.read_correction(&mut cursor, &mut output, 1001);
            assert_eq!(fresh.valid_frames, 240);
            assert_eq!(output, [[0.125, -0.0625]; 240]);
        }
    }

    #[test]
    fn wrap_overwrite_stereo_and_stale_worker_are_bounded() {
        let bus = bus();
        let mut cursor = 0;
        bus.write(&vec![[0.25, -0.5]; CAPACITY * 3], 10);
        let mut frames = [[9.0; 2]; 240];
        bus.read(&mut cursor, &mut frames, 11);
        assert_eq!(frames, [[0.25, -0.5]; 240]);
        assert_eq!(cursor, (CAPACITY * 2 + 240) as u64);
        bus.read(&mut cursor, &mut frames, 51);
        assert_eq!(frames, [[0.0; 2]; 240]);
        assert_eq!(cursor, bus.cursor());
    }
    #[test]
    fn fresh_attachment_never_replays_previous_samples() {
        let bus = bus();
        bus.write(&[[1.0, -1.0]; 240], 1);
        let mut cursor = bus.cursor();
        let mut frames = [[2.0; 2]; 240];
        bus.read(&mut cursor, &mut frames, 2);
        assert_eq!(frames, [[0.0; 2]; 240]);
        bus.write(&[[0.1, -0.2]; 240], 3);
        bus.read(&mut cursor, &mut frames, 3);
        assert_eq!(frames, [[0.1, -0.2]; 240]);
    }

    #[test]
    fn simultaneous_reader_and_writer_pause_discards_old_audio_once() {
        let bus = bus();
        bus.write(&[[0.75, -0.25]; 960], 0);
        bus.write(&[[0.125, -0.5]; 240], 1000);
        let mut cursor = 0;
        let mut frames = [[9.0; 2]; 480];
        let stats = bus.read_counted(&mut cursor, &mut frames, 1001);
        assert_eq!(frames[..240], [[0.125, -0.5]; 240]);
        assert_eq!(frames[240..], [[0.0; 2]; 240]);
        assert_eq!(
            stats,
            ReadStats {
                dropped_frames: 960,
                underrun_frames: 240,
                queued_frames: 0
            }
        );
        assert_eq!(cursor, 1200);
        assert_eq!(
            bus.read_counted(&mut cursor, &mut frames, 1002)
                .dropped_frames,
            0
        );
        bus.write(&[[0.25, -0.125]; 240], 2000);
        let stats = bus.read_counted(&mut cursor, &mut frames[..240], 2000);
        assert_eq!(frames[..240], [[0.25, -0.125]; 240]);
        assert_eq!(stats.dropped_frames, 0);
    }

    #[test]
    fn continuity_gap_boundary_preserves_forty_ms_and_fences_forty_one() {
        for gap in [40, 41] {
            let bus = bus();
            bus.write(&[[0.75, -0.25]; 240], 100);
            bus.write(&[[0.125, -0.5]; 240], 100 + gap);
            let mut cursor = 0;
            let mut frames = [[0.0; 2]; 240];
            let stats = bus.read_counted(&mut cursor, &mut frames, 100 + gap);
            if gap == 40 {
                assert_eq!(frames, [[0.75, -0.25]; 240]);
                assert_eq!(stats.dropped_frames, 0);
            } else {
                assert_eq!(frames, [[0.125, -0.5]; 240]);
                assert_eq!(stats.dropped_frames, 240);
            }
        }
    }

    #[test]
    fn each_reader_discards_only_its_own_unread_pre_pause_prefix() {
        let bus = bus();
        bus.write(&[[0.75, -0.25]; CAPACITY * 2], 0);
        bus.write(&[[0.125, -0.5]; 240], 1000);
        for mut cursor in [0, 240, 1920, 3840] {
            let expected_drop = 3840 - cursor;
            let mut frames = [[0.0; 2]; 240];
            let stats = bus.read_counted(&mut cursor, &mut frames, 1000);
            assert_eq!(frames, [[0.125, -0.5]; 240]);
            assert_eq!(u64::from(stats.dropped_frames), expected_drop);
            assert_eq!(cursor, 4080);
        }
    }

    #[test]
    fn a_resume_at_each_read_boundary_yields_silence_then_fresh_audio() {
        for boundary in [
            ReadBoundary::Cursor,
            ReadBoundary::Heartbeat,
            ReadBoundary::Floor,
            ReadBoundary::Copied,
        ] {
            let bus = bus();
            bus.write(&[[0.75, -0.25]; 960], 0);
            let mut cursor = 0;
            let mut frames = [[9.0; 2]; 240];
            let stats = bus.read_observed(&mut cursor, &mut frames, 1, |point| {
                if point == boundary {
                    bus.write(&[[0.125, -0.5]; 240], 1000);
                }
            });
            assert_eq!(frames, [[0.0; 2]; 240]);
            assert_eq!(stats.dropped_frames, 960);
            assert_eq!(cursor, 960);
            let stats = bus.read_counted(&mut cursor, &mut frames, 1001);
            assert_eq!(frames, [[0.125, -0.5]; 240]);
            assert_eq!(stats.dropped_frames, 0);
        }
    }
    #[test]
    fn reader_first_resume_discards_a_copied_block_before_the_writer_can_publish_a_floor() {
        use std::cell::Cell;
        for boundary in [
            ReadBoundary::Cursor,
            ReadBoundary::Heartbeat,
            ReadBoundary::Floor,
            ReadBoundary::Copied,
        ] {
            let bus = bus();
            bus.write(&[[0.75, -0.25]; 960], 0);
            let clock = Cell::new(1);
            let mut cursor = 0;
            let mut frames = [[9.0; 2]; 240];
            let stats = bus.read_clocked(
                &mut cursor,
                &mut frames,
                1,
                || clock.get(),
                |point| {
                    if point == boundary {
                        clock.set(1000);
                    }
                },
            );
            assert_eq!(frames, [[0.0; 2]; 240]);
            assert_eq!(cursor, 960);
            assert_eq!(stats.dropped_frames, 960);
            assert_eq!(stats.underrun_frames, 240);
            assert_eq!(bus.continuity_floor.load(SeqCst), 0);
            bus.write(&[[0.25, -0.125]; 240], 1001);
            bus.read_counted(&mut cursor, &mut frames, 1001);
            assert_eq!(frames, [[0.25, -0.125]; 240]);
        }
    }
    #[test]
    fn a_reader_pause_is_detected_when_the_writer_keeps_running_without_discontinuity() {
        use std::cell::Cell;
        let bus = bus();
        bus.write(&[[0.75, -0.25]; 960], 0);
        let clock = Cell::new(1);
        let mut cursor = 0;
        let mut frames = [[9.0; 2]; 240];
        let stats = bus.read_clocked(
            &mut cursor,
            &mut frames,
            1,
            || clock.get(),
            |point| {
                if point == ReadBoundary::Copied {
                    for tick in 1..=200 {
                        bus.write(&[[0.25, -0.125]; 240], tick * 5);
                    }
                    clock.set(1000);
                }
            },
        );
        assert_eq!(frames, [[0.0; 2]; 240]);
        assert_eq!(bus.continuity_floor.load(SeqCst), 0);
        assert_eq!(cursor, 960);
        assert_eq!(stats.dropped_frames, 960);
        assert!(cursor <= bus.cursor());
        bus.read_counted(&mut cursor, &mut frames, 1001);
        assert_eq!(frames, [[0.25, -0.125]; 240]);
    }
    #[test]
    fn completion_freshness_boundary_keeps_forty_ms_but_rejects_forty_one() {
        for elapsed in [40, 41] {
            let bus = bus();
            bus.write(&[[0.75, -0.25]; 240], 100);
            let mut cursor = 0;
            let mut frames = [[0.0; 2]; 240];
            let stats = bus.read_clocked(&mut cursor, &mut frames, 100, || 100 + elapsed, |_| {});
            if elapsed == 40 {
                assert_eq!(frames, [[0.75, -0.25]; 240]);
                assert_eq!(stats.dropped_frames, 0);
            } else {
                assert_eq!(frames, [[0.0; 2]; 240]);
                assert_eq!(stats.dropped_frames, 240);
            }
            assert_eq!(cursor, 240);
        }
    }

    #[test]
    fn previous_bus_layout_is_rejected_and_counter_conversion_is_precise() {
        let bus = bus();
        bus.version.store(1, SeqCst);
        assert!(!bus.valid());
        assert_eq!(counter_millis(9_999, 10_000_000), 0);
        assert_eq!(counter_millis(10_000, 10_000_000), 1);
        assert_eq!(counter_millis(409_999, 10_000_000), 40);
        assert_eq!(counter_millis(410_000, 10_000_000), 41);
        assert_eq!(counter_millis(3_579_545, 3_579_545), 1000);
        assert_eq!(counter_millis(u64::MAX, 1), u64::MAX);
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SimulatedReader {
        cursor: u64,
        last_sample: u64,
        hash: u64,
        received: u64,
        dropped: u64,
        underrun: u64,
        max_queued: u32,
        final_queued: u32,
        stale_silence_callbacks: u32,
        gap_callbacks: u64,
        first_gap_ns: Option<u64>,
    }

    // A virtual-clock harness for the real Bus implementation. It models a
    // 5 ms writer, independent 10 ms readers, deterministic scheduling jitter,
    // clock skew, a 60 ms producer stall and an optional 80 ms reader stall.
    // This exercises transport policy, not Windows scheduling or audibility.
    fn simulate(skew_ppm: i64, producer_stall: bool, reader_stall: bool) -> [SimulatedReader; 2] {
        simulate_seconds(skew_ppm, producer_stall, reader_stall, 60)
    }

    fn simulate_seconds(
        skew_ppm: i64,
        producer_stall: bool,
        reader_stall: bool,
        seconds: u64,
    ) -> [SimulatedReader; 2] {
        let bus = bus();
        let mut readers = std::array::from_fn(|_| SimulatedReader::default());
        let mut next_writer = 0;
        let mut writer_tick = 0u64;
        let mut next_reader = [10_000_000u64; 2];
        let periods = [
            (10_000_000_000_000i64 / (1_000_000 + skew_ppm)) as u64,
            10_000_000,
        ];
        let mut sample = 0u64;
        let mut reader_paused = false;
        loop {
            let now = next_writer.min(next_reader[0]).min(next_reader[1]);
            if now > seconds * 1_000_000_000 {
                break;
            }
            if now == next_writer {
                if !producer_stall || !(5_000_000_000..5_060_000_000).contains(&now) {
                    let frames: [[f32; 2]; 240] = std::array::from_fn(|_| {
                        sample += 1;
                        // Encode exact positive finite bit-pattern markers so
                        // long fixtures do not hit f32's integer precision limit.
                        let marker = f32::from_bits(0x3f00_0000 + sample as u32);
                        [marker, -marker]
                    });
                    bus.write(&frames, now / 1_000_000);
                }
                writer_tick += 1;
                next_writer = writer_tick * 5_000_000
                    + if writer_tick.is_multiple_of(13) {
                        2_000_000
                    } else {
                        0
                    };
            }
            for index in 0..2 {
                if now != next_reader[index] {
                    continue;
                }
                if index == 0 && reader_stall && !reader_paused && now >= 10_000_000_000 {
                    next_reader[index] += 80_000_000;
                    reader_paused = true;
                    continue;
                }
                let reader = &mut readers[index];
                let mut frames = [[0.0; 2]; 480];
                let stats = bus.read_counted(&mut reader.cursor, &mut frames, now / 1_000_000);
                reader.dropped += u64::from(stats.dropped_frames);
                reader.underrun += u64::from(stats.underrun_frames);
                reader.max_queued = reader.max_queued.max(stats.queued_frames);
                reader.final_queued = stats.queued_frames;
                if stats.dropped_frames != 0 || stats.underrun_frames != 0 {
                    reader.gap_callbacks += 1;
                    reader.first_gap_ns.get_or_insert(now);
                }
                assert!(stats.queued_frames as usize <= CAPACITY);
                assert!(reader.cursor <= bus.cursor());
                if producer_stall && (5_045_000_000..5_060_000_000).contains(&now) {
                    assert!(frames.iter().all(|frame| *frame == [0.0; 2]));
                    reader.stale_silence_callbacks += 1;
                }
                for frame in frames {
                    assert_eq!(frame[0], -frame[1]);
                    if frame[0] != 0.0 {
                        let marker = u64::from(frame[0].to_bits() - 0x3f00_0000);
                        assert!(marker > reader.last_sample, "overdue samples replayed");
                        reader.last_sample = marker;
                        reader.hash = reader.hash.wrapping_mul(16_777_619) ^ marker;
                        reader.received += 1;
                    }
                }
                next_reader[index] += periods[index];
            }
        }
        for reader in &mut readers {
            // Compare reader lag at the same virtual instant, rather than at
            // different last-callback phases within the writer's 5 ms block.
            reader.final_queued = (bus.cursor() - reader.cursor).min(CAPACITY as u64) as u32;
            assert!(
                reader.last_sample > sample - CAPACITY as u64,
                "reader did not recover"
            );
        }
        readers
    }

    #[test]
    fn independent_clocks_keep_transport_bounded_but_do_not_eliminate_drift() {
        let nominal = simulate(0, false, false);
        for skew in [100, 500] {
            let faster = simulate(skew, false, false);
            let slower = simulate(-skew, false, false);
            assert_eq!(faster[1], nominal[1]);
            assert_eq!(slower[1], nominal[1]);
            assert!(faster[0].underrun > nominal[0].underrun);
            assert!(
                slower[0].final_queued > nominal[0].final_queued,
                "skew {skew}, slower {:?}, nominal {:?}",
                slower[0],
                nominal[0]
            );
            assert!(slower[0].max_queued as usize <= CAPACITY);
        }
    }

    #[test]
    #[ignore = "manual 600-second virtual-clock baseline; no devices or DSP correction"]
    fn measure_long_session_clock_skew() {
        let nominal = simulate_seconds(0, false, false, 600);
        println!(
            "600 virtual seconds; real Bus; writer 240 frames/5 ms with 2 ms jitter every 13 ticks; readers 480 frames/10 ms; peer nominal"
        );
        println!(
            "ppm,underrun_frames,dropped_frames,gap_or_drop_callbacks,max_queued_frames,final_queued_frames,first_gap_seconds"
        );
        for skew in [-500, -100, -30, 0, 30, 100, 500] {
            let result = simulate_seconds(skew, false, false, 600);
            assert_eq!(result[1], nominal[1], "one reader changed its peer");
            let reader = &result[0];
            assert!(reader.max_queued as usize <= CAPACITY);
            if skew >= 100 {
                assert!(reader.underrun > nominal[0].underrun);
            }
            if skew <= -100 {
                assert!(reader.dropped > nominal[0].dropped);
            }
            println!(
                "{skew},{},{},{},{},{},{}",
                reader.underrun,
                reader.dropped,
                reader.gap_callbacks,
                reader.max_queued,
                reader.final_queued,
                reader
                    .first_gap_ns
                    .map_or_else(|| "none".into(), |time| format!("{:.3}", time as f64 / 1e9))
            );
        }
    }

    #[test]
    fn scheduling_stalls_recover_without_replay_or_affecting_peer_readers() {
        let baseline = simulate(500, true, false);
        let paused = simulate(500, true, true);
        assert_eq!(
            baseline[1], paused[1],
            "one reader's pause affected its peer"
        );
        assert!(paused[0].dropped > baseline[0].dropped);
        assert!(
            paused
                .iter()
                .all(|reader| reader.stale_silence_callbacks > 0)
        );
        assert!(paused.iter().all(|reader| reader.received > 2_000_000));
    }
    #[test]
    fn concurrent_overwrite_never_tears_stereo_frames() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let bus: Arc<Bus> = Arc::from(bus());
        let writer: Arc<Bus> = bus.clone();
        let done = Arc::new(AtomicBool::new(false));
        let writer_done = done.clone();
        let thread = std::thread::spawn(move || {
            for index in 1..100_000 {
                let value = (index % 1000) as f32 / 1000.0;
                writer.write(&[[value, -value]], 1);
            }
            writer_done.store(true, Ordering::Release);
        });
        let mut cursor = 0;
        let mut frames = [[0.0; 2]; 240];
        while !done.load(Ordering::Acquire) {
            bus.read(&mut cursor, &mut frames, 1);
            assert!(frames.iter().all(|frame| frame[0] == -frame[1]));
        }
        thread.join().unwrap();
    }
}
