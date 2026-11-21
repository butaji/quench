//! HTTP/2 framing (RFC 9113 §4, §6): the frame header, the frame types, flags, error
//! codes and settings, each as the wire value its name owns.

/// The client connection preface (RFC 9113 §3.4).
pub(super) const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
/// A frame header: 24-bit length, type, flags and 31-bit stream identifier.
pub(super) const HEADER_LENGTH: usize = 9;
const LENGTH_OCTETS: usize = 3;
/// The reserved bit ahead of every stream identifier and window increment.
pub(super) const RESERVED_BIT: u32 = 1 << 31;
/// The largest flow-control window (RFC 9113 §6.9.1).
pub(super) const MAX_WINDOW: i64 = (1 << 31) - 1;
/// The window every connection and stream starts with (RFC 9113 §6.9.2).
pub(super) const DEFAULT_WINDOW: u32 = 65_535;
/// The bounds of SETTINGS_MAX_FRAME_SIZE (RFC 9113 §6.5.2).
pub(super) const MIN_MAX_FRAME_SIZE: u32 = 16_384;
pub(super) const MAX_MAX_FRAME_SIZE: u32 = (1 << 24) - 1;
/// A PING frame's payload length.
pub(super) const PING_LENGTH: usize = 8;
/// A SETTINGS entry: a 16-bit identifier and a 32-bit value.
const SETTING_ID_LENGTH: usize = size_of::<u16>();
pub(super) const SETTING_LENGTH: usize = SETTING_ID_LENGTH + size_of::<u32>();
/// A GOAWAY payload opens with the last stream id, then the error code.
pub(super) const GOAWAY_LAST_STREAM_LENGTH: usize = size_of::<u32>();
/// The payload lengths of fixed-size frames.
pub(super) const PRIORITY_LENGTH: usize = 5;
pub(super) const RST_STREAM_LENGTH: usize = 4;
pub(super) const WINDOW_UPDATE_LENGTH: usize = 4;
pub(super) const GOAWAY_FIXED_LENGTH: usize = 8;
pub(super) const PUSH_PROMISE_ID_LENGTH: usize = 4;
pub(super) const ALTSVC_ORIGIN_LENGTH: usize = 2;
pub(super) const ORIGIN_ENTRY_LENGTH: usize = 2;

/// The frame types (RFC 9113 §6, RFC 7838 §4, RFC 8336 §2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Data = 0x0,
    Headers = 0x1,
    Priority = 0x2,
    RstStream = 0x3,
    Settings = 0x4,
    PushPromise = 0x5,
    Ping = 0x6,
    Goaway = 0x7,
    WindowUpdate = 0x8,
    Continuation = 0x9,
    AltSvc = 0xa,
    Origin = 0xc,
}

impl Kind {
    const ALL: [Kind; 12] = [
        Self::Data,
        Self::Headers,
        Self::Priority,
        Self::RstStream,
        Self::Settings,
        Self::PushPromise,
        Self::Ping,
        Self::Goaway,
        Self::WindowUpdate,
        Self::Continuation,
        Self::AltSvc,
        Self::Origin,
    ];

    pub(super) fn from_wire(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as u8 == value)
    }
}

/// The frame flags (RFC 9113 §6).
pub(super) mod flags {
    pub(in super::super) const END_STREAM: u8 = 0x1;
    pub(in super::super) const ACK: u8 = 0x1;
    pub(in super::super) const END_HEADERS: u8 = 0x4;
    pub(in super::super) const PADDED: u8 = 0x8;
    pub(in super::super) const PRIORITY: u8 = 0x20;
}

/// The error codes (RFC 9113 §7), as nghttp2 names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ErrorCode {
    NoError = 0x0,
    ProtocolError = 0x1,
    InternalError = 0x2,
    FlowControlError = 0x3,
    SettingsTimeout = 0x4,
    StreamClosed = 0x5,
    FrameSizeError = 0x6,
    RefusedStream = 0x7,
    Cancel = 0x8,
    CompressionError = 0x9,
    ConnectError = 0xa,
    EnhanceYourCalm = 0xb,
    InadequateSecurity = 0xc,
    Http11Required = 0xd,
}

impl ErrorCode {
    pub(super) const ALL: [ErrorCode; 14] = [
        Self::NoError,
        Self::ProtocolError,
        Self::InternalError,
        Self::FlowControlError,
        Self::SettingsTimeout,
        Self::StreamClosed,
        Self::FrameSizeError,
        Self::RefusedStream,
        Self::Cancel,
        Self::CompressionError,
        Self::ConnectError,
        Self::EnhanceYourCalm,
        Self::InadequateSecurity,
        Self::Http11Required,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::NoError => "NGHTTP2_NO_ERROR",
            Self::ProtocolError => "NGHTTP2_PROTOCOL_ERROR",
            Self::InternalError => "NGHTTP2_INTERNAL_ERROR",
            Self::FlowControlError => "NGHTTP2_FLOW_CONTROL_ERROR",
            Self::SettingsTimeout => "NGHTTP2_SETTINGS_TIMEOUT",
            Self::StreamClosed => "NGHTTP2_STREAM_CLOSED",
            Self::FrameSizeError => "NGHTTP2_FRAME_SIZE_ERROR",
            Self::RefusedStream => "NGHTTP2_REFUSED_STREAM",
            Self::Cancel => "NGHTTP2_CANCEL",
            Self::CompressionError => "NGHTTP2_COMPRESSION_ERROR",
            Self::ConnectError => "NGHTTP2_CONNECT_ERROR",
            Self::EnhanceYourCalm => "NGHTTP2_ENHANCE_YOUR_CALM",
            Self::InadequateSecurity => "NGHTTP2_INADEQUATE_SECURITY",
            Self::Http11Required => "NGHTTP2_HTTP_1_1_REQUIRED",
        }
    }

    /// Node's `nameForErrorCode[code]`.
    pub(super) fn name_of(code: u32) -> Option<&'static str> {
        Self::ALL.into_iter().find(|error| *error as u32 == code).map(Self::name)
    }
}

/// The settings identifiers (RFC 9113 §6.5.2, RFC 8441 §3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SettingId {
    HeaderTableSize = 0x1,
    EnablePush = 0x2,
    MaxConcurrentStreams = 0x3,
    InitialWindowSize = 0x4,
    MaxFrameSize = 0x5,
    MaxHeaderListSize = 0x6,
    EnableConnectProtocol = 0x8,
}

impl SettingId {
    /// In identifier order, the order SETTINGS payloads list them in.
    pub(super) const ALL: [SettingId; 7] = [
        Self::HeaderTableSize,
        Self::EnablePush,
        Self::MaxConcurrentStreams,
        Self::InitialWindowSize,
        Self::MaxFrameSize,
        Self::MaxHeaderListSize,
        Self::EnableConnectProtocol,
    ];

    pub(super) fn from_wire(value: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|id| *id as u16 == value)
    }
}

/// A frame header as read from or written to the wire.
#[derive(Clone, Copy, Debug)]
pub(super) struct Header {
    pub(super) length: usize,
    pub(super) kind: u8,
    pub(super) flags: u8,
    pub(super) stream: u32,
}

impl Header {
    pub(super) fn read(bytes: &[u8]) -> Self {
        let length = bytes[..LENGTH_OCTETS].iter().fold(0usize, |length, &byte| (length << 8) | usize::from(byte));
        Self {
            length,
            kind: bytes[LENGTH_OCTETS],
            flags: bytes[LENGTH_OCTETS + 1],
            stream: read_u32(&bytes[LENGTH_OCTETS + 2..]) & !RESERVED_BIT,
        }
    }
}

pub(super) fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(super) fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// A SETTINGS payload of `(identifier, value)` entries, as frames and `HTTP2-Settings` carry it.
pub(super) fn settings_payload(entries: &[(u16, u32)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(entries.len() * SETTING_LENGTH);
    for &(id, value) in entries {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

/// The `(identifier, value)` entries of a SETTINGS payload whose length is a multiple of
/// `SETTING_LENGTH`.
pub(super) fn settings_entries(payload: &[u8]) -> impl Iterator<Item = (u16, u32)> + '_ {
    payload
        .as_chunks::<SETTING_LENGTH>()
        .0
        .iter()
        .map(|entry| (read_u16(entry), read_u32(&entry[SETTING_ID_LENGTH..])))
}

/// Append a frame of `kind` with `payload` to `out`.
pub(super) fn write(out: &mut Vec<u8>, kind: Kind, flags: u8, stream: u32, payload: &[u8]) {
    let length = (payload.len() as u32).to_be_bytes();
    out.extend_from_slice(&length[length.len() - LENGTH_OCTETS..]);
    out.push(kind as u8);
    out.push(flags);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
}
