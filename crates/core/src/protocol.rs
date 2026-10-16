//! Private, bounded backend protocol. Never log frames or parser errors.
use crate::model::{AppError, Capture, ErrorCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
#[cfg(windows)]
use std::path::Path;
use std::{
    fmt,
    io::{BufRead, Write},
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const VERSION: u8 = 1;
pub const MAX_LINE_BYTES: usize = 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame<T> {
    pub version: u8,
    pub generation: u32,
    pub sequence: u64,
    pub body: T,
}

// Init contains a private path. Even accidental debug formatting is redacted.
impl<T> fmt::Debug for Frame<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("version", &self.version)
            .field("generation", &self.generation)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

// Closed vocabulary: no library error strings or remote payloads cross stdout.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Failure {
    Configuration,
    Authentication,
    Permission,
    Capture,
    ApplicationCapture,
    SourceExited,
    Connection,
    Protocol,
}

impl Failure {
    pub fn error(self) -> AppError {
        let (code, message) = match self {
            Self::Configuration => (
                ErrorCode::Configuration,
                "The bot configuration is unavailable.",
            ),
            Self::Authentication => (ErrorCode::Authentication, "The bot authentication failed."),
            Self::Permission => (
                ErrorCode::PermissionDenied,
                "The bot cannot use the selected voice channel.",
            ),
            Self::Capture => (
                ErrorCode::CaptureFailed,
                "The selected audio source is unavailable.",
            ),
            Self::ApplicationCapture => (
                ErrorCode::CaptureFailed,
                "Application capture is unavailable or stopped unexpectedly. Use a recording input or another running application.",
            ),
            Self::SourceExited => (
                ErrorCode::InvalidSelection,
                "The selected application exited or is unavailable. Refresh sources and select its new instance.",
            ),
            Self::Connection => (
                ErrorCode::ConnectionFailed,
                "The voice connection ended. Press Connect to retry.",
            ),
            Self::Protocol => (ErrorCode::Internal, "The engine protocol failed."),
        };
        AppError {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    Io,
    Oversized,
    Malformed,
    Version,
    Generation,
    Sequence,
    Invalid,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid engine protocol.")
    }
}
impl std::error::Error for ProtocolError {}

pub struct Validator {
    generation: u32,
    sequence: u64,
}
impl Validator {
    pub fn new(generation: u32) -> Result<Self, ProtocolError> {
        if generation == 0 {
            return Err(ProtocolError::Generation);
        }
        Ok(Self {
            generation,
            sequence: 0,
        })
    }
    pub fn accept<T>(&mut self, frame: &Frame<T>) -> Result<(), ProtocolError> {
        if frame.version != VERSION {
            return Err(ProtocolError::Version);
        }
        if frame.generation != self.generation {
            return Err(ProtocolError::Generation);
        }
        if frame.sequence <= self.sequence {
            return Err(ProtocolError::Sequence);
        }
        self.sequence = frame.sequence;
        Ok(())
    }
}

pub fn decode<T: DeserializeOwned>(line: &[u8]) -> Result<Frame<T>, ProtocolError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::Oversized);
    }
    serde_json::from_slice(line).map_err(|_| ProtocolError::Malformed)
}

pub fn encode<T: Serialize>(frame: &Frame<T>) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = serde_json::to_vec(frame).map_err(|_| ProtocolError::Malformed)?;
    if bytes.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::Oversized);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn write<T: Serialize>(writer: &mut impl Write, frame: &Frame<T>) -> Result<(), ProtocolError> {
    writer
        .write_all(&encode(frame)?)
        .and_then(|_| writer.flush())
        .map_err(|_| ProtocolError::Io)
}

/// Allocates at most one bounded frame, even for a stream without newlines.
pub fn read<T: DeserializeOwned>(
    reader: &mut impl BufRead,
) -> Result<Option<Frame<T>>, ProtocolError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| ProtocolError::Io)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(ProtocolError::Malformed)
            };
        }
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |p| p + 1);
        let complete = available[count - 1] == b'\n';
        let payload = count - usize::from(complete);
        if line.len() + payload > MAX_LINE_BYTES {
            return Err(ProtocolError::Oversized);
        }
        line.extend_from_slice(&available[..payload]);
        reader.consume(count);
        if complete {
            return decode(&line).map(Some);
        }
    }
}

pub async fn read_async<T: DeserializeOwned>(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> Result<Option<Frame<T>>, ProtocolError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|_| ProtocolError::Io)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(ProtocolError::Malformed)
            };
        }
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |p| p + 1);
        let complete = available[count - 1] == b'\n';
        let payload = count - usize::from(complete);
        if line.len() + payload > MAX_LINE_BYTES {
            return Err(ProtocolError::Oversized);
        }
        line.extend_from_slice(&available[..payload]);
        reader.consume(count);
        if complete {
            return decode(&line).map(Some);
        }
    }
}

pub fn discord_id(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok().filter(|id| *id != 0)
}

#[cfg(windows)]
pub(crate) fn read_token(path: &Path) -> Result<String, AppError> {
    crate::storage::load_token(path)
}

/// Capture adapters may have diagnostic strings; only bounded measurements cross the pipe.
pub fn clean_capture(mut capture: Capture) -> Capture {
    capture.message = None;
    capture.level_dbfs = clean_level(capture.level_dbfs);
    capture.output_level_dbfs = clean_level(capture.output_level_dbfs);
    capture
}

fn clean_level(level: Option<f32>) -> Option<f32> {
    level
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(-160.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::protocol::{GatewayCommand as Command, VoiceStatus as Status};
    use std::io::Cursor;

    #[test]
    fn both_meter_levels_are_sanitized_and_roundtrip_as_nullable_scalars() {
        for (level, expected) in [
            (None, None),
            (Some(f32::NAN), None),
            (Some(f32::INFINITY), None),
            (Some(f32::NEG_INFINITY), None),
            (Some(-200.0), Some(-160.0)),
            (Some(2.0), Some(0.0)),
            (Some(-6.0), Some(-6.0)),
        ] {
            let capture = clean_capture(Capture {
                level_dbfs: level,
                output_level_dbfs: level,
                message: Some("private diagnostic".into()),
                ..crate::model::ControlSnapshot::default().capture
            });
            assert_eq!(capture.level_dbfs, expected);
            assert_eq!(capture.output_level_dbfs, expected);
            assert_eq!(capture.message, None);
            let frame = Frame {
                version: VERSION,
                generation: 1,
                sequence: 1,
                body: Status::Meter { capture },
            };
            let encoded = encode(&frame).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(
                json["body"]["capture"]["outputLevelDbfs"],
                serde_json::to_value(expected).unwrap()
            );
            assert_eq!(
                read::<Status>(&mut Cursor::new(encoded))
                    .unwrap()
                    .unwrap()
                    .body,
                frame.body
            );
        }
    }
    fn frame(sequence: u64) -> Frame<Command> {
        Frame {
            version: VERSION,
            generation: 7,
            sequence,
            body: Command::Stop,
        }
    }
    #[test]
    fn rejects_version_generation_replay_and_zero() {
        assert!(Validator::new(0).is_err());
        let mut v = Validator::new(7).unwrap();
        assert!(v.accept(&frame(1)).is_ok());
        assert_eq!(v.accept(&frame(1)), Err(ProtocolError::Sequence));
        let mut f = frame(2);
        f.version = 2;
        assert_eq!(v.accept(&f), Err(ProtocolError::Version));
        f.version = VERSION;
        f.generation = 8;
        assert_eq!(v.accept(&f), Err(ProtocolError::Generation));
        assert!(v.accept(&frame(3)).is_ok());
    }
    #[test]
    fn malformed_oversized_partial_and_unknown_are_redacted() {
        for bytes in [
            b"private-token\n".to_vec(),
            vec![b'x'; MAX_LINE_BYTES + 1],
            b"{\"private-token\":true}\n".to_vec(),
            b"{}".to_vec(),
        ] {
            let error = read::<Command>(&mut Cursor::new(bytes)).unwrap_err();
            assert!(!error.to_string().contains("private-token"));
        }
    }
    #[tokio::test]
    async fn async_parser_bounds_unterminated_lines_and_roundtrips() {
        let bytes = encode(&frame(1)).unwrap();
        let parsed = read_async::<Command>(&mut bytes.as_slice())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parsed.sequence, 1);
        let bytes = vec![b'x'; MAX_LINE_BYTES + 1];
        assert_eq!(
            read_async::<Command>(&mut bytes.as_slice())
                .await
                .unwrap_err(),
            ProtocolError::Oversized
        );
    }
    #[test]
    fn private_paths_never_appear_in_debug() {
        let init = Command::Init {
            token_file: "/private/secret".into(),
        };
        assert!(
            !format!(
                "{:?}",
                Frame {
                    version: VERSION,
                    generation: 1,
                    sequence: 1,
                    body: init.clone()
                }
            )
            .contains("secret")
        );
        assert!(discord_id("+1").is_none());
        assert!(discord_id("0").is_none());
    }
    #[cfg(windows)]
    #[test]
    fn token_reader_requires_encryption_and_redacts_failures() {
        let path =
            std::env::temp_dir().join(format!("echocord-token-synthetic-{}", std::process::id()));
        for invalid in [
            b"synthetic-token".as_slice(),
            b"ECHOCORD-TOKEN",
            b"ECHOCORD-TOKEN\0",
        ] {
            std::fs::write(&path, invalid).unwrap();
            let failure = read_token(&path).unwrap_err();
            assert!(!failure.message.contains(path.to_str().unwrap()));
            assert!(!failure.message.contains("synthetic-token"));
        }
        crate::storage::save_token(&path, "synthetic-token").unwrap();
        assert_eq!(read_token(&path).unwrap(), "synthetic-token");
        std::fs::remove_file(path).unwrap();
    }
}
