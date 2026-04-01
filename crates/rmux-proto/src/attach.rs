//! Attach-stream message codec shared by client and server.

use crate::{RmuxError, TerminalSize, DEFAULT_MAX_FRAME_LENGTH};
use serde::{Deserialize, Serialize};

const DATA_TAG: u8 = 1;
const RESIZE_TAG: u8 = 2;
const LOCK_TAG: u8 = 3;
const UNLOCK_TAG: u8 = 4;
const SUSPEND_TAG: u8 = 5;
const DETACH_KILL_TAG: u8 = 6;
const DETACH_EXEC_TAG: u8 = 7;
const KEYSTROKE_TAG: u8 = 8;
const KEY_DISPATCHED_TAG: u8 = 9;
const DATA_HEADER_LEN: usize = 5;
const RESIZE_FRAME_LEN: usize = 5;
const SINGLE_TAG_FRAME_LEN: usize = 1;

/// Typed attach-stream input captured from an attached client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachedKeystroke {
    bytes: Vec<u8>,
}

impl AttachedKeystroke {
    /// Creates a typed keystroke from the terminal byte sequence read by the client.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Returns the terminal byte sequence carried by this typed keystroke.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Structured acknowledgement returned after the server receives a typed keystroke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDispatched {
    byte_len: u32,
    consumed: bool,
}

impl KeyDispatched {
    /// Creates a consumed dispatch acknowledgement for the received keystroke byte length.
    #[must_use]
    pub fn new(byte_len: u32) -> Self {
        Self {
            byte_len,
            consumed: true,
        }
    }

    /// Creates a dispatch acknowledgement for key bytes forwarded to the pane.
    #[must_use]
    pub fn forwarded(byte_len: u32) -> Self {
        Self {
            byte_len,
            consumed: false,
        }
    }

    /// Returns the number of key bytes acknowledged by the server.
    #[must_use]
    pub fn byte_len(&self) -> u32 {
        self.byte_len
    }

    /// Returns whether the server consumed the key before it reached the pane.
    #[must_use]
    pub fn consumed(&self) -> bool {
        self.consumed
    }
}

/// All message types supported after the attach upgrade boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachMessage {
    /// Raw pane I/O bytes.
    Data(Vec<u8>),
    /// Typed key input from an attached client.
    Keystroke(AttachedKeystroke),
    /// Structured acknowledgement for a typed key input message.
    KeyDispatched(KeyDispatched),
    /// A terminal resize event.
    Resize(TerminalSize),
    /// A request for the client to run the configured lock command locally.
    Lock(String),
    /// A notification that the local lock command has completed.
    Unlock,
    /// A request for the client to suspend itself and later resume in raw mode.
    Suspend,
    /// A request for the client to terminate itself after detaching.
    DetachKill,
    /// A request for the client to run a shell command locally before detaching.
    DetachExec(String),
}

/// Encodes a single attach-stream message.
pub fn encode_attach_message(message: &AttachMessage) -> Result<Vec<u8>, RmuxError> {
    match message {
        AttachMessage::Data(bytes) => encode_data_message(bytes),
        AttachMessage::Keystroke(keystroke) => encode_structured_message(KEYSTROKE_TAG, keystroke),
        AttachMessage::KeyDispatched(response) => {
            encode_structured_message(KEY_DISPATCHED_TAG, response)
        }
        AttachMessage::Resize(size) => Ok(encode_resize_message(*size)),
        AttachMessage::Lock(command) => encode_data_like_message(LOCK_TAG, command.as_bytes()),
        AttachMessage::Unlock => Ok(vec![UNLOCK_TAG]),
        AttachMessage::Suspend => Ok(vec![SUSPEND_TAG]),
        AttachMessage::DetachKill => Ok(vec![DETACH_KILL_TAG]),
        AttachMessage::DetachExec(command) => {
            encode_data_like_message(DETACH_EXEC_TAG, command.as_bytes())
        }
    }
}

/// Incremental decoder for attach-stream messages.
#[derive(Debug, Clone)]
pub struct AttachFrameDecoder {
    max_data_length: usize,
    buffer: Vec<u8>,
}

impl AttachFrameDecoder {
    /// Creates a decoder with the default maximum attach payload length.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_data_length(DEFAULT_MAX_FRAME_LENGTH)
    }

    /// Creates a decoder with a custom maximum attach payload length.
    #[must_use]
    pub fn with_max_data_length(max_data_length: usize) -> Self {
        Self {
            max_data_length,
            buffer: Vec::new(),
        }
    }

    /// Appends raw attach-stream bytes to the decoder buffer.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Attempts to decode the next full attach-stream message.
    pub fn next_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        let Some(&tag) = self.buffer.first() else {
            return Ok(None);
        };

        match tag {
            DATA_TAG => self.next_data_message(),
            RESIZE_TAG => self.next_resize_message(),
            LOCK_TAG => self.next_lock_message(),
            UNLOCK_TAG => self.next_unlock_message(),
            SUSPEND_TAG => self.next_suspend_message(),
            DETACH_KILL_TAG => self.next_detach_kill_message(),
            DETACH_EXEC_TAG => self.next_detach_exec_message(),
            KEYSTROKE_TAG => self.next_keystroke_message(),
            KEY_DISPATCHED_TAG => self.next_key_dispatched_message(),
            other => {
                self.buffer.clear();
                Err(RmuxError::Decode(format!(
                    "unknown attach-stream message tag {other}"
                )))
            }
        }
    }

    fn next_data_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        if self.buffer.len() < DATA_HEADER_LEN {
            return Ok(None);
        }

        let length = u32::from_le_bytes(
            self.buffer[1..DATA_HEADER_LEN]
                .try_into()
                .map_err(|_| RmuxError::Decode("invalid attach data header".to_owned()))?,
        ) as usize;

        if length > self.max_data_length {
            self.buffer.clear();
            return Err(RmuxError::FrameTooLarge {
                length,
                maximum: self.max_data_length,
            });
        }

        let required = DATA_HEADER_LEN + length;
        if self.buffer.len() < required {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..required).collect();
        Ok(Some(AttachMessage::Data(frame[DATA_HEADER_LEN..].to_vec())))
    }

    fn next_resize_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        if self.buffer.len() < RESIZE_FRAME_LEN {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..RESIZE_FRAME_LEN).collect();
        let cols = u16::from_le_bytes(
            frame[1..3]
                .try_into()
                .map_err(|_| RmuxError::Decode("invalid attach resize columns".to_owned()))?,
        );
        let rows = u16::from_le_bytes(
            frame[3..5]
                .try_into()
                .map_err(|_| RmuxError::Decode("invalid attach resize rows".to_owned()))?,
        );

        Ok(Some(AttachMessage::Resize(TerminalSize { cols, rows })))
    }

    fn next_lock_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        self.next_data_like_message(LOCK_TAG).map(|message| {
            message.map(|bytes| AttachMessage::Lock(String::from_utf8_lossy(&bytes).into_owned()))
        })
    }

    fn next_unlock_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        if self.buffer.len() < SINGLE_TAG_FRAME_LEN {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..SINGLE_TAG_FRAME_LEN).collect();
        if frame[0] != UNLOCK_TAG {
            self.buffer.clear();
            return Err(RmuxError::Decode("invalid attach unlock frame".to_owned()));
        }

        Ok(Some(AttachMessage::Unlock))
    }

    fn next_suspend_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        if self.buffer.len() < SINGLE_TAG_FRAME_LEN {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..SINGLE_TAG_FRAME_LEN).collect();
        if frame[0] != SUSPEND_TAG {
            self.buffer.clear();
            return Err(RmuxError::Decode("invalid attach suspend frame".to_owned()));
        }

        Ok(Some(AttachMessage::Suspend))
    }

    fn next_detach_kill_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        if self.buffer.len() < SINGLE_TAG_FRAME_LEN {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..SINGLE_TAG_FRAME_LEN).collect();
        if frame[0] != DETACH_KILL_TAG {
            self.buffer.clear();
            return Err(RmuxError::Decode(
                "invalid attach detach-kill frame".to_owned(),
            ));
        }

        Ok(Some(AttachMessage::DetachKill))
    }

    fn next_detach_exec_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        self.next_data_like_message(DETACH_EXEC_TAG).map(|message| {
            message.map(|bytes| {
                AttachMessage::DetachExec(String::from_utf8_lossy(&bytes).into_owned())
            })
        })
    }

    fn next_keystroke_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        self.next_structured_message(KEYSTROKE_TAG)
            .map(|message| message.map(AttachMessage::Keystroke))
    }

    fn next_key_dispatched_message(&mut self) -> Result<Option<AttachMessage>, RmuxError> {
        self.next_structured_message(KEY_DISPATCHED_TAG)
            .map(|message| message.map(AttachMessage::KeyDispatched))
    }

    fn next_structured_message<T>(&mut self, tag: u8) -> Result<Option<T>, RmuxError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let Some(bytes) = self.next_data_like_message(tag)? else {
            return Ok(None);
        };

        bincode::deserialize(&bytes)
            .map(Some)
            .map_err(|error| RmuxError::Decode(format!("invalid attach structured frame: {error}")))
    }

    fn next_data_like_message(&mut self, tag: u8) -> Result<Option<Vec<u8>>, RmuxError> {
        if self.buffer.len() < DATA_HEADER_LEN {
            return Ok(None);
        }

        let length = u32::from_le_bytes(
            self.buffer[1..DATA_HEADER_LEN]
                .try_into()
                .map_err(|_| RmuxError::Decode("invalid attach data header".to_owned()))?,
        ) as usize;

        if length > self.max_data_length {
            self.buffer.clear();
            return Err(RmuxError::FrameTooLarge {
                length,
                maximum: self.max_data_length,
            });
        }

        let required = DATA_HEADER_LEN + length;
        if self.buffer.len() < required {
            return Ok(None);
        }

        let frame: Vec<u8> = self.buffer.drain(..required).collect();
        if frame[0] != tag {
            self.buffer.clear();
            return Err(RmuxError::Decode(
                "invalid attach data-like frame".to_owned(),
            ));
        }
        Ok(Some(frame[DATA_HEADER_LEN..].to_vec()))
    }
}

impl Default for AttachFrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_data_message(bytes: &[u8]) -> Result<Vec<u8>, RmuxError> {
    encode_data_like_message(DATA_TAG, bytes)
}

fn encode_data_like_message(tag: u8, bytes: &[u8]) -> Result<Vec<u8>, RmuxError> {
    if bytes.len() > DEFAULT_MAX_FRAME_LENGTH {
        return Err(RmuxError::FrameTooLarge {
            length: bytes.len(),
            maximum: DEFAULT_MAX_FRAME_LENGTH,
        });
    }

    let length = u32::try_from(bytes.len()).map_err(|_| RmuxError::FrameTooLarge {
        length: bytes.len(),
        maximum: u32::MAX as usize,
    })?;

    let mut frame = Vec::with_capacity(DATA_HEADER_LEN + bytes.len());
    frame.push(tag);
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(bytes);
    Ok(frame)
}

fn encode_structured_message<T>(tag: u8, message: &T) -> Result<Vec<u8>, RmuxError>
where
    T: Serialize,
{
    let bytes = bincode::serialize(message)
        .map_err(|error| RmuxError::Encode(format!("invalid attach structured frame: {error}")))?;
    encode_data_like_message(tag, &bytes)
}

fn encode_resize_message(size: TerminalSize) -> Vec<u8> {
    let mut frame = Vec::with_capacity(RESIZE_FRAME_LEN);
    frame.push(RESIZE_TAG);
    frame.extend_from_slice(&size.cols.to_le_bytes());
    frame.extend_from_slice(&size.rows.to_le_bytes());
    frame
}

#[cfg(test)]
mod tests {
    use super::{
        encode_attach_message, AttachFrameDecoder, AttachMessage, AttachedKeystroke, KeyDispatched,
    };
    use crate::{RmuxError, TerminalSize};

    #[test]
    fn data_messages_round_trip() {
        let message = AttachMessage::Data(b"hello".to_vec());
        let encoded = encode_attach_message(&message).expect("encode attach message");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach message"),
            Some(message)
        );
        assert_eq!(
            decoder.next_message().expect("buffer should be empty"),
            None
        );
    }

    #[test]
    fn resize_messages_round_trip() {
        let message = AttachMessage::Resize(TerminalSize {
            cols: 120,
            rows: 40,
        });
        let encoded = encode_attach_message(&message).expect("encode attach resize");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach resize"),
            Some(message)
        );
    }

    #[test]
    fn keystroke_messages_round_trip() {
        let message = AttachMessage::Keystroke(AttachedKeystroke::new(b"\x1b[A".to_vec()));
        let encoded = encode_attach_message(&message).expect("encode attach keystroke");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach keystroke"),
            Some(message)
        );
    }

    #[test]
    fn key_dispatched_messages_round_trip() {
        let message = AttachMessage::KeyDispatched(KeyDispatched::new(3));
        let encoded = encode_attach_message(&message).expect("encode attach key dispatch ack");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder
                .next_message()
                .expect("decode attach key dispatch ack"),
            Some(message)
        );
    }

    #[test]
    fn lock_messages_round_trip() {
        let message = AttachMessage::Lock("lock-command".to_owned());
        let encoded = encode_attach_message(&message).expect("encode attach lock");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach lock"),
            Some(message)
        );
    }

    #[test]
    fn unlock_messages_round_trip() {
        let message = AttachMessage::Unlock;
        let encoded = encode_attach_message(&message).expect("encode attach unlock");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach unlock"),
            Some(message)
        );
    }

    #[test]
    fn decoder_handles_fragmented_messages() {
        let message = AttachMessage::Data(b"fragmented".to_vec());
        let encoded = encode_attach_message(&message).expect("encode attach message");
        let mut decoder = AttachFrameDecoder::new();

        decoder.push_bytes(&encoded[..3]);
        assert_eq!(
            decoder
                .next_message()
                .expect("partial message should not fail"),
            None
        );

        decoder.push_bytes(&encoded[3..]);
        assert_eq!(
            decoder.next_message().expect("fragment should decode"),
            Some(message)
        );
    }

    #[test]
    fn suspend_messages_round_trip() {
        let message = AttachMessage::Suspend;
        let encoded = encode_attach_message(&message).expect("encode attach suspend");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach suspend"),
            Some(message)
        );
    }

    #[test]
    fn detach_kill_messages_round_trip() {
        let message = AttachMessage::DetachKill;
        let encoded = encode_attach_message(&message).expect("encode attach detach-kill");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach detach-kill"),
            Some(message)
        );
    }

    #[test]
    fn detach_exec_messages_round_trip() {
        let message = AttachMessage::DetachExec("exec /bin/bash".to_owned());
        let encoded = encode_attach_message(&message).expect("encode attach detach-exec");
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&encoded);

        assert_eq!(
            decoder.next_message().expect("decode attach detach-exec"),
            Some(message)
        );
    }

    #[test]
    fn decoder_rejects_unknown_tags() {
        let mut decoder = AttachFrameDecoder::new();
        decoder.push_bytes(&[10, 0, 0, 0, 0]);

        assert_eq!(
            decoder.next_message(),
            Err(RmuxError::Decode(
                "unknown attach-stream message tag 10".to_owned()
            ))
        );
    }
}
