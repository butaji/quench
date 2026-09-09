//! Rust-owned HTTP/2 wire facts and the small state machine shared by the
//! endpoint and public HTTP/2 layers.
//!
//! This module deliberately stops at framing and session invariants.  Header
//! compression and JavaScript stream objects are separate layers; keeping the
//! wire state here means those layers consume one canonical representation of
//! frame boundaries instead of each reparsing TCP chunks.

use std::collections::{HashMap, HashSet};

pub const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16_384;
pub const MAX_MAX_FRAME_SIZE: u32 = 16_777_215;

/// Hidden socket/server facts used at the transport edge.  They are data
/// markers, not fixture names or dispatch hooks.
pub const CLIENT_MARKER: &str = "\0quench:http2-client";
pub const SERVER_MARKER: &str = "\0quench:http2-server";
pub const PROTOCOL_ERROR_MARKER: &str = "\0quench:http2-protocol-error";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameType {
    Data,
    Headers,
    Priority,
    RstStream,
    Settings,
    PushPromise,
    Ping,
    GoAway,
    WindowUpdate,
    Continuation,
    Unknown(u8),
}

impl From<u8> for FrameType {
    fn from(value: u8) -> Self {
        match value {
            0x0 => Self::Data,
            0x1 => Self::Headers,
            0x2 => Self::Priority,
            0x3 => Self::RstStream,
            0x4 => Self::Settings,
            0x5 => Self::PushPromise,
            0x6 => Self::Ping,
            0x7 => Self::GoAway,
            0x8 => Self::WindowUpdate,
            0x9 => Self::Continuation,
            other => Self::Unknown(other),
        }
    }
}

impl FrameType {
    pub const fn wire(self) -> u8 {
        match self {
            Self::Data => 0x0,
            Self::Headers => 0x1,
            Self::Priority => 0x2,
            Self::RstStream => 0x3,
            Self::Settings => 0x4,
            Self::PushPromise => 0x5,
            Self::Ping => 0x6,
            Self::GoAway => 0x7,
            Self::WindowUpdate => 0x8,
            Self::Continuation => 0x9,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub length: u32,
    pub kind: FrameType,
    pub flags: u8,
    pub stream_id: u32,
}

impl FrameHeader {
    pub const SIZE: usize = 9;

    pub fn decode(bytes: &[u8]) -> Result<Option<Self>, ProtocolError> {
        if bytes.len() < Self::SIZE {
            return Ok(None);
        }
        let length = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        let raw_stream = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
        if raw_stream & 0x8000_0000 != 0 {
            return Err(ProtocolError::ReservedStreamBit);
        }
        Ok(Some(Self {
            length,
            kind: FrameType::from(bytes[3]),
            flags: bytes[4],
            stream_id: raw_stream & 0x7fff_ffff,
        }))
    }

    pub fn encode(self, output: &mut Vec<u8>) {
        let length = self.length.min(0x00ff_ffff);
        output.extend_from_slice(&[
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
            self.kind.wire(),
            self.flags,
        ]);
        output.extend_from_slice(&(self.stream_id & 0x7fff_ffff).to_be_bytes());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: FrameType, flags: u8, stream_id: u32, payload: Vec<u8>) -> Self {
        Self {
            header: FrameHeader {
                length: payload.len() as u32,
                kind,
                flags,
                stream_id,
            },
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(FrameHeader::SIZE + self.payload.len());
        self.header.encode(&mut output);
        output.extend_from_slice(&self.payload);
        output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidPreface,
    ReservedStreamBit,
    FrameTooLarge(u32),
    InvalidFrameLength(FrameType, usize),
    InvalidStream(FrameType),
    InvalidSettings,
    InvalidWindowIncrement,
    FlowControlError,
    InvalidHeaderBlock,
    FrameAfterGoAway,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    Idle,
    Open,
    HalfClosedRemote,
    HalfClosedLocal,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stream {
    pub state: StreamState,
    pub recv_window: i64,
    pub send_window: i64,
}

pub struct Session {
    role: Role,
    input: Vec<u8>,
    preface_offset: usize,
    decoder: hpack::Decoder<'static>,
    encoder: hpack::Encoder<'static>,
    pending_headers: HashMap<u32, Vec<u8>>,
    announced_headers: HashSet<u32>,
    /// PUSH_PROMISE headers must survive a later response HEADERS block for
    /// the same promised stream ID. Keep that wire event separate from the
    /// ordinary latest-headers map.
    push_promises: HashMap<u32, Vec<(Vec<u8>, Vec<u8>)>>,
    pub max_frame_size: u32,
    pub goaway: bool,
    pub streams: HashMap<u32, Stream>,
    /// Connection-level receive window, kept as a signed fact so overflow
    /// validation cannot wrap before it reaches the protocol boundary.
    connection_recv_window: i64,
    /// Decoded header fields, keyed by stream ID.
    pub headers: HashMap<u32, Vec<(Vec<u8>, Vec<u8>)>>,
}

impl Session {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            input: Vec::new(),
            preface_offset: if role == Role::Client {
                CONNECTION_PREFACE.len()
            } else {
                0
            },
            decoder: hpack::Decoder::new(),
            encoder: hpack::Encoder::new(),
            pending_headers: HashMap::new(),
            announced_headers: HashSet::new(),
            push_promises: HashMap::new(),
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            goaway: false,
            streams: HashMap::new(),
            connection_recv_window: 65_535,
            headers: HashMap::new(),
        }
    }

    /// Feed arbitrary TCP chunks and return complete frames.  Partial
    /// prefaces/headers/payloads remain buffered until the next call.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, ProtocolError> {
        self.input.extend_from_slice(bytes);
        self.consume_preface()?;
        let mut frames = Vec::new();
        while self.input.len() >= FrameHeader::SIZE {
            let Some(header) = FrameHeader::decode(&self.input)? else {
                break;
            };
            if header.length > self.max_frame_size {
                return Err(ProtocolError::FrameTooLarge(header.length));
            }
            let total = FrameHeader::SIZE + header.length as usize;
            if self.input.len() < total {
                break;
            }
            let payload = self.input[FrameHeader::SIZE..total].to_vec();
            let frame = Frame { header, payload };
            self.validate_and_apply(&frame)?;
            self.input.drain(..total);
            frames.push(frame);
        }
        Ok(frames)
    }

    fn consume_preface(&mut self) -> Result<(), ProtocolError> {
        if self.preface_offset == CONNECTION_PREFACE.len() {
            return Ok(());
        }
        let available = self.input.len().min(CONNECTION_PREFACE.len());
        if self.input[..available] != CONNECTION_PREFACE[..available] {
            return Err(ProtocolError::InvalidPreface);
        }
        if available < CONNECTION_PREFACE.len() {
            return Ok(());
        }
        self.input.drain(..CONNECTION_PREFACE.len());
        self.preface_offset = CONNECTION_PREFACE.len();
        Ok(())
    }

    fn validate_and_apply(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if self.goaway {
            return Err(ProtocolError::FrameAfterGoAway);
        }
        if !self.pending_headers.is_empty() && frame.header.kind != FrameType::Continuation {
            return Err(ProtocolError::InvalidStream(frame.header.kind));
        }
        let stream_zero = frame.header.stream_id == 0;
        let length = frame.payload.len();
        match frame.header.kind {
            FrameType::Settings => self.apply_settings(frame),
            FrameType::Ping if !stream_zero && length == 8 => {
                Err(ProtocolError::InvalidStream(FrameType::Ping))
            }
            FrameType::Ping if stream_zero && length == 8 => Ok(()),
            FrameType::Ping => Err(ProtocolError::InvalidFrameLength(FrameType::Ping, length)),
            FrameType::Priority if stream_zero || length != 5 => Err(
                ProtocolError::InvalidFrameLength(FrameType::Priority, length),
            ),
            FrameType::RstStream if stream_zero || length != 4 => Err(
                ProtocolError::InvalidFrameLength(FrameType::RstStream, length),
            ),
            FrameType::WindowUpdate if length != 4 => Err(ProtocolError::InvalidFrameLength(
                FrameType::WindowUpdate,
                length,
            )),
            FrameType::WindowUpdate
                if u32::from_be_bytes(frame.payload.clone().try_into().unwrap()) & 0x7fff_ffff
                    == 0 =>
            {
                Err(ProtocolError::InvalidWindowIncrement)
            }
            FrameType::WindowUpdate if stream_zero => self.apply_window_update(frame),
            FrameType::GoAway if !stream_zero && length >= 8 => {
                Err(ProtocolError::InvalidStream(FrameType::GoAway))
            }
            FrameType::GoAway if stream_zero && length >= 8 => {
                self.goaway = true;
                Ok(())
            }
            FrameType::GoAway => Err(ProtocolError::InvalidFrameLength(FrameType::GoAway, length)),
            FrameType::Data | FrameType::Headers | FrameType::Continuation if stream_zero => {
                Err(ProtocolError::InvalidStream(frame.header.kind))
            }
            FrameType::PushPromise if stream_zero => {
                Err(ProtocolError::InvalidStream(FrameType::PushPromise))
            }
            FrameType::Data
            | FrameType::Headers
            | FrameType::PushPromise
            | FrameType::Continuation => self.apply_stream_frame(frame),
            _ => Ok(()),
        }
    }

    fn apply_settings(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if frame.header.stream_id != 0 || frame.payload.len() % 6 != 0 {
            return Err(ProtocolError::InvalidSettings);
        }
        if frame.header.flags & 0x1 != 0 && !frame.payload.is_empty() {
            return Err(ProtocolError::InvalidSettings);
        }
        for setting in frame.payload.chunks_exact(6) {
            let id = u16::from_be_bytes([setting[0], setting[1]]);
            let value = u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]);
            match id {
                1 => self.decoder.set_max_table_size(value as usize),
                2 if value > 1 => return Err(ProtocolError::InvalidSettings),
                4 if value > 0x7fff_ffff => return Err(ProtocolError::InvalidSettings),
                5 if !(16_384..=MAX_MAX_FRAME_SIZE).contains(&value) => {
                    return Err(ProtocolError::InvalidSettings)
                }
                5 => self.max_frame_size = value,
                _ => {}
            }
        }
        Ok(())
    }

    fn apply_window_update(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let increment = (u32::from_be_bytes(frame.payload[..4].try_into().unwrap())
            & 0x7fff_ffff) as i64;
        if self.connection_recv_window.saturating_add(increment) > 0x7fff_ffff {
            return Err(ProtocolError::FlowControlError);
        }
        self.connection_recv_window += increment;
        Ok(())
    }

    fn apply_stream_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let id = frame.header.stream_id;
        let stream = self.streams.entry(id).or_insert(Stream {
            state: StreamState::Open,
            recv_window: 65_535,
            send_window: 65_535,
        });
        if stream.state == StreamState::Closed {
            return Err(ProtocolError::InvalidStream(frame.header.kind));
        }
        if frame.header.flags & 0x1 != 0 {
            stream.state = StreamState::HalfClosedRemote;
        }
        match frame.header.kind {
            FrameType::Headers => self.apply_headers(id, frame)?,
            FrameType::Continuation => self.apply_continuation(id, frame)?,
            FrameType::PushPromise => {
                if frame.payload.len() < 4 {
                    return Err(ProtocolError::InvalidFrameLength(
                        FrameType::PushPromise,
                        frame.payload.len(),
                    ));
                }
                let promised_id =
                    u32::from_be_bytes(frame.payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                self.decode_headers(promised_id, &frame.payload[4..])?;
                if let Some(headers) = self.headers.get(&promised_id).cloned() {
                    self.push_promises.insert(promised_id, headers);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_headers(&mut self, id: u32, frame: &Frame) -> Result<(), ProtocolError> {
        let payload = header_block_payload(frame)?;
        if frame.header.flags & 0x4 != 0 {
            self.decode_headers(id, payload)
        } else {
            self.pending_headers.insert(id, payload.to_vec());
            Ok(())
        }
    }

    fn apply_continuation(&mut self, id: u32, frame: &Frame) -> Result<(), ProtocolError> {
        let Some(mut block) = self.pending_headers.remove(&id) else {
            return Err(ProtocolError::InvalidStream(FrameType::Continuation));
        };
        block.extend_from_slice(&frame.payload);
        if frame.header.flags & 0x4 == 0 {
            self.pending_headers.insert(id, block);
            return Ok(());
        }
        self.decode_headers(id, &block)
    }

    fn decode_headers(&mut self, id: u32, block: &[u8]) -> Result<(), ProtocolError> {
        let decoded = self
            .decoder
            .decode(block)
            .map_err(|_| ProtocolError::InvalidHeaderBlock)?;
        self.headers.insert(id, decoded);
        Ok(())
    }

    /// Encode a header block using the session's compression context.  Literal
    /// strings are valid HPACK; the decoder accepts indexed and Huffman forms.
    pub fn encode_headers(&mut self, headers: &[(&[u8], &[u8])]) -> Vec<u8> {
        self.encoder.encode(headers.iter().copied())
    }

    /// Return each newly completed header block once, leaving the decoded
    /// representation available for diagnostics and protocol consumers.
    pub fn take_new_headers(&mut self) -> Vec<(u32, Vec<(Vec<u8>, Vec<u8>)>)> {
        let ids = self
            .headers
            .keys()
            .copied()
            .filter(|id| self.announced_headers.insert(*id))
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| self.headers.get(&id).cloned().map(|headers| (id, headers)))
            .collect()
    }

    pub fn take_push_promises(&mut self) -> HashMap<u32, Vec<(Vec<u8>, Vec<u8>)>> {
        std::mem::take(&mut self.push_promises)
    }

    pub fn pending_bytes(&self) -> usize {
        self.input.len()
    }

    pub fn role(&self) -> Role {
        self.role
    }
}

fn header_block_payload(frame: &Frame) -> Result<&[u8], ProtocolError> {
    let mut start = 0;
    let mut end = frame.payload.len();
    if frame.header.flags & 0x8 != 0 {
        let Some(&padding) = frame.payload.first() else {
            return Err(ProtocolError::InvalidFrameLength(FrameType::Headers, 0));
        };
        start = 1;
        if padding as usize > end.saturating_sub(start) {
            return Err(ProtocolError::InvalidFrameLength(
                FrameType::Headers,
                frame.payload.len(),
            ));
        }
        end -= padding as usize;
    }
    if frame.header.flags & 0x20 != 0 {
        if end.saturating_sub(start) < 5 {
            return Err(ProtocolError::InvalidFrameLength(
                FrameType::Headers,
                frame.payload.len(),
            ));
        }
        start += 5;
    }
    Ok(&frame.payload[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_preserves_wire_header() {
        let frame = Frame::new(FrameType::Data, 1, 3, b"hello".to_vec());
        let mut session = Session::new(Role::Client);
        let decoded = session.feed(&frame.encode()).unwrap();
        assert_eq!(decoded, vec![frame]);
    }

    #[test]
    fn server_accepts_split_preface_and_frame() {
        let frame = Frame::new(FrameType::Settings, 0, 0, Vec::new());
        let wire = [CONNECTION_PREFACE, &frame.encode()].concat();
        let mut session = Session::new(Role::Server);
        assert!(session.feed(&wire[..7]).unwrap().is_empty());
        assert_eq!(session.feed(&wire[7..]).unwrap(), vec![frame]);
    }

    #[test]
    fn settings_update_frame_limit() {
        let payload = [
            5u16.to_be_bytes().as_slice(),
            32_768u32.to_be_bytes().as_slice(),
        ]
        .concat();
        let frame = Frame::new(FrameType::Settings, 0, 0, payload);
        let mut session = Session::new(Role::Client);
        session.feed(&frame.encode()).unwrap();
        assert_eq!(session.max_frame_size, 32_768);
    }

    #[test]
    fn decodes_indexed_and_literal_header_block() {
        let block = [0x82, 0x84, 0x00, 0x01, b'x', 0x01, b'y'];
        let frame = Frame::new(FrameType::Headers, 0x4, 1, block.to_vec());
        let mut session = Session::new(Role::Client);
        session.feed(&frame.encode()).unwrap();
        assert_eq!(
            session.headers.get(&1).unwrap(),
            &vec![
                (b":method".to_vec(), b"GET".to_vec()),
                (b":path".to_vec(), b"/".to_vec()),
                (b"x".to_vec(), b"y".to_vec()),
            ]
        );
    }

    #[test]
    fn decodes_huffman_string_literal() {
        let block = [
            0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff,
        ];
        let frame = Frame::new(FrameType::Headers, 0x4, 1, block.to_vec());
        let mut session = Session::new(Role::Client);
        session.feed(&frame.encode()).unwrap();
        assert_eq!(
            session.headers.get(&1).unwrap(),
            &vec![(b":authority".to_vec(), b"www.example.com".to_vec())]
        );
    }

    #[test]
    fn assembles_continuation_and_round_trips_encoder() {
        let mut session = Session::new(Role::Client);
        let block = session.encode_headers(&[(b":method", b"GET"), (b":path", b"/")]);
        let split = block.len() / 2;
        let first = Frame::new(FrameType::Headers, 0, 3, block[..split].to_vec());
        let second = Frame::new(FrameType::Continuation, 0x4, 3, block[split..].to_vec());
        assert_eq!(session.feed(&first.encode()).unwrap().len(), 1);
        session.feed(&second.encode()).unwrap();
        assert_eq!(
            session.headers.get(&3).unwrap(),
            &vec![
                (b":method".to_vec(), b"GET".to_vec()),
                (b":path".to_vec(), b"/".to_vec()),
            ]
        );
    }

    #[test]
    fn rejects_padded_header_without_payload() {
        let frame = Frame::new(FrameType::Headers, 0xC, 1, vec![1]);
        let mut session = Session::new(Role::Client);
        assert!(matches!(
            session.feed(&frame.encode()),
            Err(ProtocolError::InvalidFrameLength(FrameType::Headers, 1))
        ));
    }
}
