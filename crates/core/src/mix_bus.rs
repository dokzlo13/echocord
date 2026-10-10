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
impl Bus {
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
        frames.fill([0.0; 2]);
        let latest = self.cursor();
        if !self.valid() || now_ms.saturating_sub(self.heartbeat_ms.load(SeqCst)) > 40 {
            *cursor = latest;
            return;
        }
        *cursor = (*cursor).max(latest.saturating_sub(CAPACITY as u64));
        for frame in frames {
            if *cursor >= latest {
                break;
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
            }
            *cursor += 1;
        }
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
