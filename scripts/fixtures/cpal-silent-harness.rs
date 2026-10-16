//! Device-free mocks around the actual patched CPAL packet function.
#![allow(dead_code, non_snake_case)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    mem::{self, size_of},
    ptr,
    rc::Rc,
    slice,
};

struct CountingAllocator;
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
fn allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Debug)]
struct Error;
impl Error {
    fn with_message(_: ErrorKind, _: &'static str) -> Self {
        Self
    }
}
enum ErrorKind {
    Xrun,
    BackendError,
}
impl From<ErrorKind> for Error {
    fn from(_: ErrorKind) -> Self {
        Self
    }
}
trait ResultExt<T> {
    fn context(self, _: &str) -> Result<T, Error>;
}
impl<T> ResultExt<T> for Result<T, Error> {
    fn context(self, _: &str) -> Result<T, Error> {
        self
    }
}
type ErrorCallbackArc = Cell<usize>;
fn try_emit_error(errors: &ErrorCallbackArc, _: Error) -> bool {
    errors.set(errors.get() + 1);
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SampleFormat {
    I8,
    I16,
    I24,
    I32,
    I64,
    U8,
    U16,
    U24,
    U32,
    U64,
    F32,
    F64,
    DsdU8,
}
impl SampleFormat {
    fn sample_size(self) -> usize {
        match self {
            Self::I8 | Self::U8 | Self::DsdU8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I24 | Self::I32 | Self::U24 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }
    fn is_int(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I24 | Self::I32 | Self::I64
        )
    }
    fn is_uint(self) -> bool {
        matches!(
            self,
            Self::U8 | Self::U16 | Self::U24 | Self::U32 | Self::U64
        )
    }
    fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
    fn is_dsd(self) -> bool {
        self == Self::DsdU8
    }
}
trait Sample {
    const EQUILIBRIUM: Self;
}
impl Sample for u16 {
    const EQUILIBRIUM: Self = 1 << 15;
}
impl Sample for u32 {
    const EQUILIBRIUM: Self = 1 << 31;
}
impl Sample for u64 {
    const EQUILIBRIUM: Self = 1 << 63;
}
#[derive(Clone, Copy)]
#[repr(transparent)]
struct U24(u32);
impl Sample for U24 {
    const EQUILIBRIUM: Self = Self(1 << 23);
}

struct StreamInner {
    bytes_per_frame: u16,
    sample_format: SampleFormat,
    max_frames_in_buffer: u32,
    clock_fails: bool,
}
struct Data {
    data: *mut (),
    len: usize,
    format: SampleFormat,
}
impl Data {
    unsafe fn from_parts(data: *mut (), len: usize, format: SampleFormat) -> Self {
        Self { data, len, format }
    }
}
struct InputCallbackInfo {
    timestamp: InputStreamTimestamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StreamInstant {
    seconds: u64,
    nanos: u32,
}
impl StreamInstant {
    const ZERO: Self = Self::new(0, 0);
    const fn new(seconds: u64, nanos: u32) -> Self {
        Self { seconds, nanos }
    }
}
struct InputStreamTimestamp {
    capture: StreamInstant,
    callback: StreamInstant,
}
const CALLBACK_CLOCK: StreamInstant = StreamInstant::new(4321, 123_400);
fn clock_position(stream: &StreamInner) -> Result<(StreamInstant, u64), Error> {
    if stream.clock_fails {
        Err(Error)
    } else {
        Ok((CALLBACK_CLOCK, 42))
    }
}
// __UPSTREAM_INPUT_TIMESTAMP__

mod Audio {
    use super::*;
    pub struct Flag(pub i32);
    pub const AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY: Flag = Flag(1);
    pub const AUDCLNT_BUFFERFLAGS_SILENT: Flag = Flag(2);
    pub const AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR: Flag = Flag(4);
    pub struct State {
        pub payload: Vec<u64>,
        pub flags: u32,
        pub frames: u32,
        pub null: bool,
        pub timestamp: u64,
        pub releases: Cell<usize>,
        pub release_fails: bool,
        pub clock_fails: bool,
    }
    #[derive(Clone)]
    pub struct IAudioCaptureClient(pub Rc<State>);
    impl IAudioCaptureClient {
        pub unsafe fn GetNextPacketSize(&self) -> Result<u32, Error> {
            Ok(if self.0.releases.get() == 0 {
                self.0.frames.max(1)
            } else {
                0
            })
        }
        pub unsafe fn GetBuffer(
            &self,
            data: *mut *mut u8,
            frames: *mut u32,
            flags: *mut u32,
            position: Option<*mut u64>,
            qpc: Option<*mut u64>,
        ) -> Result<(), Error> {
            unsafe {
                *frames = self.0.frames;
                if self.0.frames == 0 {
                    return Ok(());
                } // Other outputs intentionally untouched.
                *data = if self.0.null {
                    ptr::null_mut()
                } else {
                    self.0.payload.as_ptr().cast_mut().cast()
                };
                *flags = self.0.flags;
                if let Some(p) = position {
                    *p = 42;
                }
                if let Some(t) = qpc {
                    *t = self.0.timestamp;
                }
            }
            Ok(())
        }
        pub unsafe fn ReleaseBuffer(&self, frames: u32) -> Result<(), Error> {
            assert_eq!(frames, self.0.frames);
            self.0.releases.set(self.0.releases.get() + 1);
            if self.0.release_fails {
                Err(Error)
            } else {
                Ok(())
            }
        }
    }
}

// __UPSTREAM_EQUILIBRIUM__
// __PATCHED_PROCESS_INPUT__

fn state(flags: u32, null: bool) -> Audio::State {
    Audio::State {
        payload: vec![u64::MAX; 8],
        flags,
        frames: 2,
        null,
        timestamp: 123,
        releases: Cell::new(0),
        release_fails: false,
        clock_fails: false,
    }
}

fn inspect(
    format: SampleFormat,
    packet: Audio::State,
    max_frames: u32,
    scratch_words: usize,
) -> (
    Result<(), Error>,
    Rc<Audio::State>,
    [u8; 64],
    usize,
    usize,
    usize,
) {
    let stream = StreamInner {
        bytes_per_frame: (format.sample_size() * 2) as u16,
        sample_format: format,
        max_frames_in_buffer: max_frames,
        clock_fails: packet.clock_fails,
    };
    let packet = Rc::new(packet);
    let mut scratch = vec![0u64; scratch_words];
    let errors = Cell::new(0);
    let mut observed = [0u8; 64];
    let mut count = 0;
    let mut callbacks = 0;
    let client = Audio::IAudioCaptureClient(packet.clone());
    ALLOCATIONS.with(|allocations| allocations.set(Some(0)));
    let result = process_input(
        &stream,
        client,
        &mut |data, info| {
            let expected_capture = if packet.flags & 4 != 0 {
                StreamInstant::ZERO
            } else {
                let ns = u128::from(packet.timestamp) * 100;
                StreamInstant::new((ns / 1_000_000_000) as u64, (ns % 1_000_000_000) as u32)
            };
            assert_eq!(info.timestamp.capture, expected_capture);
            assert_eq!(info.timestamp.callback, CALLBACK_CLOCK);
            assert_ne!(info.timestamp.capture, info.timestamp.callback);
            assert_eq!(data.data as usize % format.sample_size(), 0);
            let bytes = unsafe {
                slice::from_raw_parts(data.data.cast::<u8>(), data.len * format.sample_size())
            };
            observed[..bytes.len()].copy_from_slice(bytes);
            count = bytes.len();
            callbacks += 1;
        },
        &errors,
        &mut scratch,
    );
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    // Real CPAL error construction is outside this mock. The successful packet
    // path uses the actual processing/equilibrium code and must not allocate.
    if result.is_ok() {
        assert_eq!(allocations, 0, "successful packet processing allocated");
    }
    (result, packet, observed, count, callbacks, errors.get())
}

#[test]
fn silent_packets_ignore_nonzero_or_null_payload_for_every_pcm_format() {
    for format in [
        SampleFormat::I8,
        SampleFormat::I16,
        SampleFormat::I24,
        SampleFormat::I32,
        SampleFormat::I64,
        SampleFormat::U8,
        SampleFormat::U16,
        SampleFormat::U24,
        SampleFormat::U32,
        SampleFormat::U64,
        SampleFormat::F32,
        SampleFormat::F64,
    ] {
        for null in [false, true] {
            let (result, packet, actual, len, callbacks, _) = inspect(format, state(2, null), 2, 8);
            result.unwrap();
            assert_eq!(callbacks, 1);
            assert_eq!(packet.releases.get(), 1);
            assert_eq!(len, format.sample_size() * 4);
            let expected: Vec<u8> = match format {
                SampleFormat::U8 => vec![0x80],
                SampleFormat::U16 => 32768u16.to_ne_bytes().to_vec(),
                SampleFormat::U24 => (1u32 << 23).to_ne_bytes().to_vec(),
                SampleFormat::U32 => (1u32 << 31).to_ne_bytes().to_vec(),
                SampleFormat::U64 => (1u64 << 63).to_ne_bytes().to_vec(),
                _ => vec![0; format.sample_size()],
            };
            for sample in actual[..len].chunks_exact(format.sample_size()) {
                assert_eq!(sample, expected);
            }
            assert!(packet.payload.iter().all(|word| *word == u64::MAX));
        }
    }
}

#[test]
fn ordinary_packets_preserve_bytes_and_i24_conversion() {
    let mut input = state(0, false);
    input.payload[0] = u64::from(0.75f32.to_bits()) | (u64::from((-0.25f32).to_bits()) << 32);
    let original = input.payload[0].to_ne_bytes();
    let (result, packet, actual, _, callbacks, _) = inspect(SampleFormat::F32, input, 2, 8);
    result.unwrap();
    assert_eq!(callbacks, 1);
    assert_eq!(&actual[..8], &original);
    assert_eq!(packet.releases.get(), 1);
    let mut input = state(0, false);
    input.payload[0] = u64::from(0x12345600u32) | (u64::from(0xFEDCBA00u32) << 32);
    let (result, packet, actual, _, _, _) = inspect(SampleFormat::I24, input, 2, 8);
    result.unwrap();
    assert_eq!(
        i32::from_ne_bytes(actual[..4].try_into().unwrap()),
        0x123456
    );
    assert_eq!(
        i32::from_ne_bytes(actual[4..8].try_into().unwrap()),
        -0x12346
    );
    assert_eq!(packet.releases.get(), 1);
}

#[test]
fn every_acquired_packet_is_released_on_error_and_empty_packets_are_ignored() {
    for (frames, capacity, scratch, flags, null, clock_fails, release_fails) in [
        (3, 2, 8, 2, true, false, false), // Oversize: must not inspect null silent data.
        (2, 2, 1, 2, true, false, false), // Scratch capacity is independently bounded.
        (2, 2, 8, 0, true, false, false), // Null nonsilent packet.
        (2, 2, 8, 2, false, true, false), // Callback clock query fails.
        (2, 2, 8, 2, false, false, true), // Release failure remains an error.
    ] {
        let mut packet = state(flags, null);
        packet.frames = frames;
        packet.clock_fails = clock_fails;
        packet.release_fails = release_fails;
        let (result, packet, _, _, _, _) = inspect(SampleFormat::F32, packet, capacity, scratch);
        assert!(result.is_err());
        assert_eq!(packet.releases.get(), 1);
    }
    let mut empty = state(0, true);
    empty.frames = 0;
    let (result, packet, _, _, callbacks, _) = inspect(SampleFormat::F32, empty, 2, 8);
    result.unwrap();
    assert_eq!(callbacks, 0);
    assert_eq!(packet.releases.get(), 0);
}

#[test]
fn discontinuity_and_silence_are_independent_flags() {
    let (result, packet, actual, len, callbacks, xruns) =
        inspect(SampleFormat::F32, state(3, true), 2, 8);
    result.unwrap();
    assert_eq!(callbacks, 1);
    assert_eq!(xruns, 1);
    assert!(actual[..len].iter().all(|byte| *byte == 0));
    assert_eq!(packet.releases.get(), 1);
}

#[test]
fn uncertain_capture_time_preserves_pcm_callback_clock_and_packet_release() {
    for timestamp in [123, u64::MAX] {
        for flags in [4, 4 | 1, 4 | 2, 4 | 1 | 2] {
            let silent = flags & 2 != 0;
            let mut input = state(flags, silent);
            input.timestamp = timestamp;
            let pair = u64::from(0.75f32.to_bits()) | (u64::from((-0.25f32).to_bits()) << 32);
            input.payload[0] = pair;
            input.payload[1] = pair;
            let (result, packet, actual, len, callbacks, xruns) =
                inspect(SampleFormat::F32, input, 2, 8);
            result.unwrap();
            assert_eq!(callbacks, 1);
            assert_eq!(xruns, usize::from(flags & 1 != 0));
            assert_eq!(len, 16);
            if silent {
                assert!(actual[..len].iter().all(|byte| *byte == 0));
            } else {
                assert_eq!(&actual[..8], &pair.to_ne_bytes());
                assert_eq!(&actual[8..16], &pair.to_ne_bytes());
            }
            assert_eq!(packet.releases.get(), 1);
        }
    }
}
