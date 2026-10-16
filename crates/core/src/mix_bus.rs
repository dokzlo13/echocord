//! Bounded native-only stereo transport. The desktop parent owns a mapping handle
//! but never maps it. Every shared access is aligned atomic, including samples.
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

pub const CAPACITY: usize = 1920;
const MAGIC: u64 = 0x4543484f4d495831;
#[repr(C, align(8))]
struct Slot {
    sequence: AtomicU64,
    stereo: AtomicU64,
}
#[repr(C, align(8))]
pub struct Bus {
    magic: AtomicU64,
    version: AtomicU64,
    capacity: AtomicU64,
    cursor: AtomicU64,
    heartbeat_ms: AtomicU64,
    slots: [Slot; CAPACITY],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadStats {
    pub queued_frames: u32,
    pub dropped_frames: u32,
    pub underrun_frames: u32,
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
            slots: std::array::from_fn(|_| Slot {
                sequence: AtomicU64::new(0),
                stereo: AtomicU64::new(0),
            }),
        });
        bus.initialize();
        bus
    }
    pub fn initialize(&self) {
        self.cursor.store(0, SeqCst);
        self.version.store(1, SeqCst);
        self.capacity.store(CAPACITY as u64, SeqCst);
        self.magic.store(MAGIC, SeqCst);
    }
    pub fn valid(&self) -> bool {
        self.magic.load(SeqCst) == MAGIC
            && self.version.load(SeqCst) == 1
            && self.capacity.load(SeqCst) == CAPACITY as u64
    }
    pub fn cursor(&self) -> u64 {
        self.cursor.load(SeqCst)
    }
    pub fn write(&self, frames: &[[f32; 2]], now_ms: u64) {
        let mut cursor = self.cursor();
        for frame in frames {
            let slot = &self.slots[cursor as usize % CAPACITY];
            slot.sequence
                .store(cursor.wrapping_mul(2).wrapping_add(1), SeqCst);
            slot.stereo.store(
                u64::from(frame[0].to_bits()) | (u64::from(frame[1].to_bits()) << 32),
                SeqCst,
            );
            slot.sequence
                .store(cursor.wrapping_mul(2).wrapping_add(2), SeqCst);
            cursor += 1;
        }
        self.heartbeat_ms.store(now_ms, SeqCst);
        self.cursor.store(cursor, SeqCst);
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
        frames.fill([0.0; 2]);
        let latest = self.cursor();
        let mut stats = ReadStats::default();
        if !self.valid() || now_ms.saturating_sub(self.heartbeat_ms.load(SeqCst)) > 40 {
            stats.dropped_frames = latest.saturating_sub(*cursor).min(u32::MAX as u64) as u32;
            stats.underrun_frames = frames.len().min(u32::MAX as usize) as u32;
            *cursor = latest;
            return stats;
        }
        let oldest = latest.saturating_sub(CAPACITY as u64);
        stats.dropped_frames = oldest.saturating_sub(*cursor).min(u32::MAX as u64) as u32;
        *cursor = (*cursor).max(oldest);
        for frame in frames {
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
        stats.queued_frames = latest.saturating_sub(*cursor).min(CAPACITY as u64) as u32;
        stats
    }
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
            SystemInformation::GetTickCount64,
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
    pub fn now_ms() -> u64 {
        unsafe { GetTickCount64() }
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
    fn bus() -> Box<Bus> {
        let bus = Box::new(Bus {
            magic: AtomicU64::new(0),
            version: AtomicU64::new(0),
            capacity: AtomicU64::new(0),
            cursor: AtomicU64::new(0),
            heartbeat_ms: AtomicU64::new(0),
            slots: std::array::from_fn(|_| Slot {
                sequence: AtomicU64::new(0),
                stereo: AtomicU64::new(0),
            }),
        });
        bus.initialize();
        bus
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
    }

    // A virtual-clock harness for the real Bus implementation. It models a
    // 5 ms writer, independent 10 ms readers, deterministic scheduling jitter,
    // clock skew, a 60 ms producer stall and an optional 80 ms reader stall.
    // This exercises transport policy, not Windows scheduling or audibility.
    fn simulate(skew_ppm: i64, producer_stall: bool, reader_stall: bool) -> [SimulatedReader; 2] {
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
            if now > 60_000_000_000 {
                break;
            }
            if now == next_writer {
                if !producer_stall || !(5_000_000_000..5_060_000_000).contains(&now) {
                    let frames: [[f32; 2]; 240] = std::array::from_fn(|_| {
                        sample += 1;
                        // Exact integer markers are valid finite Bus samples;
                        // opposite channels detect tearing and channel swaps.
                        [sample as f32, -(sample as f32)]
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
                assert!(stats.queued_frames as usize <= CAPACITY);
                assert!(reader.cursor <= bus.cursor());
                if producer_stall && (5_045_000_000..5_060_000_000).contains(&now) {
                    assert!(frames.iter().all(|frame| *frame == [0.0; 2]));
                    reader.stale_silence_callbacks += 1;
                }
                for frame in frames {
                    assert_eq!(frame[0], -frame[1]);
                    if frame[0] != 0.0 {
                        let marker = frame[0] as u64;
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
