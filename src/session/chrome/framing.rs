//! Native messaging frames: a 32-bit length in native byte order, then that many bytes of
//! UTF-8 JSON. The host's stdin/stdout and the app channel use the same frames.
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};

/// The largest frame either side accepts or sends. Chrome refuses host messages over 1 MB;
/// a YouTube session is a few kilobytes.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Why a frame could not be read. Never carries the frame's content.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The stream ended cleanly before a new frame.
    Closed,
    /// The stream ended inside a frame.
    Truncated,
    TooLarge,
    /// Not UTF-8 JSON of the expected shape.
    Invalid,
    Io(io::ErrorKind),
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::UnexpectedEof => Self::Truncated,
            kind => Self::Io(kind),
        }
    }
}

/// Reads one frame's bytes.
pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, FrameError> {
    let mut length = [0; 4];
    let mut filled = 0;
    while filled < length.len() {
        match reader.read(&mut length[filled..]) {
            Ok(0) if filled == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(FrameError::TooLarge);
    }
    if length == 0 {
        return Err(FrameError::Invalid);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// Reads one frame and parses it as `T`.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, FrameError> {
    let body = read_frame(reader)?;
    serde_json::from_slice(&body).map_err(|_| FrameError::Invalid)
}

/// Writes `message` as one frame and flushes it.
pub fn write_message<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_ne_bytes());
    frame.extend_from_slice(&body);
    writer.write_all(&frame)?;
    writer.flush()
}

#[cfg(test)]
pub(crate) fn frame(body: &[u8]) -> Vec<u8> {
    let mut frame = (body.len() as u32).to_ne_bytes().to_vec();
    frame.extend_from_slice(body);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Cursor;

    #[test]
    fn messages_round_trip_with_a_native_length_prefix() {
        let mut out = Vec::new();
        write_message(&mut out, &json!({"type": "hello", "text": "é"})).unwrap();
        let length = u32::from_ne_bytes(out[..4].try_into().unwrap()) as usize;
        assert_eq!(
            length,
            out.len() - 4,
            "The length counts bytes, not characters"
        );
        write_message(&mut out, &json!({"type": "second"})).unwrap();
        let mut reader = Cursor::new(out);
        let first: Value = read_message(&mut reader).unwrap();
        assert_eq!(first["text"], "é");
        let second: Value = read_message(&mut reader).unwrap();
        assert_eq!(second["type"], "second");
        assert_eq!(
            read_message::<Value>(&mut reader).unwrap_err(),
            FrameError::Closed
        );
    }

    #[test]
    fn truncated_oversized_empty_and_bad_json_frames_are_refused() {
        let cases: [(Vec<u8>, FrameError); 6] = [
            (vec![1, 0], FrameError::Truncated),
            (
                frame(b"{\"type\":\"hello\"}")[..10].to_vec(),
                FrameError::Truncated,
            ),
            (
                ((MAX_FRAME + 1) as u32).to_ne_bytes().to_vec(),
                FrameError::TooLarge,
            ),
            (u32::MAX.to_ne_bytes().to_vec(), FrameError::TooLarge),
            (0u32.to_ne_bytes().to_vec(), FrameError::Invalid),
            (frame(b"{not json"), FrameError::Invalid),
        ];
        for (bytes, expected) in cases {
            assert_eq!(
                read_message::<Value>(&mut Cursor::new(bytes)).unwrap_err(),
                expected
            );
        }
        assert_eq!(
            read_message::<Value>(&mut Cursor::new(frame(&[0xff, 0xfe]))).unwrap_err(),
            FrameError::Invalid
        );
    }

    #[test]
    fn the_largest_frame_is_accepted_and_a_larger_message_is_not_written() {
        let text = "a".repeat(MAX_FRAME - 2);
        let mut out = Vec::new();
        write_message(&mut out, &text).unwrap();
        assert_eq!(out.len(), MAX_FRAME + 4);
        let back: String = read_message(&mut Cursor::new(out)).unwrap();
        assert_eq!(back.len(), text.len());
        let mut out = Vec::new();
        assert!(write_message(&mut out, &"a".repeat(MAX_FRAME)).is_err());
        assert!(out.is_empty());
    }
}
