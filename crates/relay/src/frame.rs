//! Length-prefixed JSON frames: a big-endian `u32` byte length, then UTF-8
//! JSON. Oversized lengths are rejected before any allocation.

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub enum FrameError {
    /// Clean end of stream at a frame boundary.
    Closed,
    TooLarge {
        length: usize,
        max: usize,
    },
    Malformed(serde_json::Error),
    Io(io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("connection closed"),
            Self::TooLarge { length, max } => write!(f, "frame of {length} bytes exceeds {max}"),
            Self::Malformed(error) => write!(f, "malformed frame: {error}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn encode<T: Serialize>(message: &T, max: usize) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(message).map_err(FrameError::Malformed)?;
    if body.len() > max {
        return Err(FrameError::TooLarge {
            length: body.len(),
            max,
        });
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub fn write_frame<W: Write, T: Serialize>(
    writer: &mut W,
    message: &T,
    max: usize,
) -> Result<(), FrameError> {
    let frame = encode(message, max)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    max: usize,
) -> Result<T, FrameError> {
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::Closed);
        }
        Err(error) => return Err(FrameError::Io(error)),
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > max {
        return Err(FrameError::TooLarge { length, max });
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(FrameError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &serde_json::json!({ "a": 1 }), 64).expect("write");
        let value: serde_json::Value = read_frame(&mut buffer.as_slice(), 64).expect("read");
        assert_eq!(value, serde_json::json!({ "a": 1 }));
    }

    #[test]
    fn rejects_oversized_length_without_reading_body() {
        let mut bytes = (10_000_000u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let result: Result<serde_json::Value, _> = read_frame(&mut bytes.as_slice(), 1024);
        assert!(matches!(result, Err(FrameError::TooLarge { .. })));
    }

    #[test]
    fn eof_at_boundary_is_closed() {
        let result: Result<serde_json::Value, _> = read_frame(&mut [].as_slice(), 64);
        assert!(matches!(result, Err(FrameError::Closed)));
    }

    #[test]
    fn refuses_to_encode_oversized_messages() {
        let result = encode(&"x".repeat(100), 16);
        assert!(matches!(result, Err(FrameError::TooLarge { .. })));
    }
}
