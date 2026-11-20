//! One HTTP/2 connection's protocol state (RFC 9113): framing, header blocks, stream states,
//! flow control, settings, pings and GOAWAY. Bytes received go in with `receive`; submissions
//! queue frames; `take_output` yields the bytes to send and `take_events` what the session
//! must surface. Priority signals are read and ignored (RFC 9113 §5.3.2).

use super::frame::{self, flags, ErrorCode, Kind, SettingId};
use super::hpack::{Decoder, Encoder, Field};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Role {
    Client,
    Server,
}

/// The settings one endpoint is bound by (RFC 9113 §6.5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Settings {
    pub(super) header_table_size: u32,
    pub(super) enable_push: bool,
    pub(super) max_concurrent_streams: u32,
    pub(super) initial_window_size: u32,
    pub(super) max_frame_size: u32,
    pub(super) max_header_list_size: u32,
    pub(super) enable_connect_protocol: bool,
}

impl Settings {
    /// The values in force before any SETTINGS frame; the unbounded limits read as the
    /// largest 32-bit value, as nghttp2 reports them.
    pub(super) const INITIAL: Settings = Settings {
        header_table_size: super::hpack::DEFAULT_TABLE_SIZE as u32,
        enable_push: true,
        max_concurrent_streams: u32::MAX,
        initial_window_size: frame::DEFAULT_WINDOW,
        max_frame_size: frame::MIN_MAX_FRAME_SIZE,
        max_header_list_size: u32::MAX,
        enable_connect_protocol: false,
    };

    fn set(&mut self, id: SettingId, value: u32) {
        match id {
            SettingId::HeaderTableSize => self.header_table_size = value,
            SettingId::EnablePush => self.enable_push = value != 0,
            SettingId::MaxConcurrentStreams => self.max_concurrent_streams = value,
            SettingId::InitialWindowSize => self.initial_window_size = value,
            SettingId::MaxFrameSize => self.max_frame_size = value,
            SettingId::MaxHeaderListSize => self.max_header_list_size = value,
            SettingId::EnableConnectProtocol => self.enable_connect_protocol = value != 0,
        }
    }

    /// Apply a SETTINGS payload's entries; unknown identifiers are ignored (RFC 9113 §6.5.2).
    fn apply(&mut self, entries: &[(u16, u32)]) {
        for &(id, value) in entries {
            if let Some(id) = SettingId::from_wire(id) {
                self.set(id, value);
            }
        }
    }
}

/// How a header block relates to its stream (nghttp2's `nghttp2_headers_category`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Category {
    Request = 0,
    Response = 1,
    PushResponse = 2,
    Headers = 3,
}

/// What the connection surfaces to its session, in the order it happened.
#[derive(Debug)]
pub(super) enum Event {
    Headers { stream: u32, category: Category, flags: u8, fields: Vec<Field> },
    Data { stream: u32, bytes: Vec<u8>, end: bool },
    /// A stream closed; `peer_reset` when by the peer's RST_STREAM.
    StreamClosed { stream: u32, code: u32, peer_reset: bool },
    /// A chunk passed to `write` was framed.
    WriteDone { stream: u32 },
    /// A stream that awaits trailers drained its data.
    WantTrailers { stream: u32 },
    RemoteSettings,
    SettingsAck { duration: f64 },
    Ping { payload: [u8; frame::PING_LENGTH] },
    PingAck { payload: [u8; frame::PING_LENGTH], duration: f64 },
    Goaway { code: u32, last_stream: u32, data: Vec<u8> },
    AltSvc { stream: u32, origin: Vec<u8>, value: Vec<u8> },
    Origin { origins: Vec<Vec<u8>> },
    /// The peer violated the protocol; a GOAWAY with `code` was sent and the connection
    /// accepts nothing more.
    ConnectionError { code: u32 },
}

/// A submission the connection cannot take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Refusal {
    /// Every stream identifier is used (NGHTTP2_ERR_STREAM_ID_NOT_AVAILABLE).
    OutOfStreams,
    /// The stream is closed or unknown (NGHTTP2_ERR_STREAM_CLOSED).
    StreamClosed,
    /// Too many SETTINGS or PING frames await acknowledgement.
    TooManyPending,
}

/// The concurrency nghttp2 assumes of a peer until its first SETTINGS arrives.
const ASSUMED_PEER_MAX_CONCURRENT_STREAMS: u32 = 100;
/// Node's default `maxOutstandingPings` and `maxOutstandingSettings`.
const MAX_OUTSTANDING: usize = 10;
/// Peer-initiated stream identifiers: odd from clients, even from servers.
const CLIENT_FIRST_STREAM: u32 = 1;
const SERVER_FIRST_STREAM: u32 = 2;
const STREAM_ID_STEP: u32 = 2;
/// The largest stream identifier (31 bits).
const MAX_STREAM_ID: u32 = !frame::RESERVED_BIT;
/// A window is replenished once half of it was consumed, as nghttp2 does.
const WINDOW_UPDATE_DIVISOR: i64 = 2;
use crate::modules::net_socket_options::MILLISECONDS_PER_SECOND;

/// nghttp2's stream states, as `Http2Stream.state.state` reports them.
#[derive(Clone, Copy)]
pub(super) enum StreamPhase {
    Idle = 1,
    Open = 2,
    ReservedLocal = 3,
    ReservedRemote = 4,
    HalfClosedLocal = 5,
    HalfClosedRemote = 6,
    Closed = 7,
}

#[derive(Default)]
struct Stream {
    send_window: i64,
    recv_unacked: i64,
    local_closed: bool,
    remote_closed: bool,
    /// Data awaiting the window, and how much of the first chunk was already framed.
    queue: VecDeque<Vec<u8>>,
    sent_of_first: usize,
    end_requested: bool,
    want_trailers: bool,
    trailers_asked: bool,
    /// The peer's final (non-informational) head arrived.
    final_head: bool,
    /// Promised by the peer (a client's pushed stream) and not yet answered.
    promised: bool,
    reserved_local: bool,
    /// A request or final response head was sent, so DATA may follow (RFC 9113 §8.1).
    head_sent: bool,
}

impl Stream {
    /// Whether flushing would frame something for this stream: data the windows allow, or
    /// the end of a drained stream.
    fn sendable(&self, connection_window: i64) -> bool {
        if self.local_closed || !self.head_sent {
            return false;
        }
        if self.queue.is_empty() {
            self.end_requested && !(self.want_trailers && self.trailers_asked)
        } else {
            self.send_window > 0 && connection_window > 0
        }
    }
}

/// A header block being read across HEADERS or PUSH_PROMISE and CONTINUATION frames.
struct Block {
    stream: u32,
    promised: Option<u32>,
    flags: u8,
    fragment: Vec<u8>,
}

pub(super) struct Connection {
    role: Role,
    pub(super) local: Settings,
    sent_settings: VecDeque<(Vec<(u16, u32)>, Instant)>,
    pub(super) remote: Settings,
    /// Whether the peer's first SETTINGS arrived, replacing what was assumed of it.
    remote_settings_received: bool,
    encoder: Encoder,
    decoder: Decoder,
    input: Vec<u8>,
    awaiting_preface: bool,
    output: Vec<u8>,
    streams: FxHashMap<u32, Stream>,
    /// The open streams' ids in the order they opened, the order their DATA is framed in.
    opened: Vec<u32>,
    next_stream_id: u32,
    last_peer_stream: u32,
    send_window: i64,
    recv_window: i64,
    recv_unacked: i64,
    block: Option<Block>,
    pings: VecDeque<Instant>,
    goaway_sent: bool,
    terminated: bool,
    events: Vec<Event>,
}

/// The numbers `Http2Session.state` reports.
pub(super) struct SessionState {
    pub(super) effective_local_window_size: f64,
    pub(super) effective_recv_data_length: f64,
    pub(super) next_stream_id: f64,
    pub(super) local_window_size: f64,
    pub(super) last_proc_stream_id: f64,
    pub(super) remote_window_size: f64,
    pub(super) outbound_queue_size: f64,
    pub(super) deflate_dynamic_table_size: f64,
    pub(super) inflate_dynamic_table_size: f64,
}

/// The numbers `Http2Stream.state` reports.
pub(super) struct StreamState {
    pub(super) phase: StreamPhase,
    pub(super) local_close: bool,
    pub(super) remote_close: bool,
    pub(super) local_window_size: f64,
}

/// The bytes a padded frame carries after its pad length and before its padding.
fn unpadded(header: &frame::Header, payload: &[u8]) -> Option<(usize, usize)> {
    if header.flags & flags::PADDED == 0 {
        return Some((0, payload.len()));
    }
    let padding = usize::from(*payload.first()?);
    let end = payload.len().checked_sub(padding)?;
    (end >= 1).then_some((1, end))
}

fn milliseconds_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND
}

impl Connection {
    /// A connection whose client preface, if it is a client, is already queued; its first
    /// frame must be the SETTINGS frame `submit_settings` queues (RFC 9113 §3.4).
    pub(super) fn new(role: Role) -> Self {
        let mut connection = Self {
            role,
            local: Settings::INITIAL,
            sent_settings: VecDeque::new(),
            remote: Settings { max_concurrent_streams: ASSUMED_PEER_MAX_CONCURRENT_STREAMS, ..Settings::INITIAL },
            remote_settings_received: false,
            encoder: Encoder::new(),
            decoder: Decoder::new(),
            input: Vec::new(),
            awaiting_preface: role == Role::Server,
            output: Vec::new(),
            streams: FxHashMap::default(),
            opened: Vec::new(),
            next_stream_id: match role {
                Role::Client => CLIENT_FIRST_STREAM,
                Role::Server => SERVER_FIRST_STREAM,
            },
            last_peer_stream: 0,
            send_window: i64::from(frame::DEFAULT_WINDOW),
            recv_window: i64::from(frame::DEFAULT_WINDOW),
            recv_unacked: 0,
            block: None,
            pings: VecDeque::new(),
            goaway_sent: false,
            terminated: false,
            events: Vec::new(),
        };
        if role == Role::Client {
            connection.output.extend_from_slice(frame::PREFACE);
        }
        connection
    }

    /// The bytes to send: the frames queued so far, then the DATA the windows allow, in the
    /// order the streams opened. Framing DATA only here lets a reset submitted meanwhile
    /// discard what its stream still queued, as nghttp2 does.
    pub(super) fn take_output(&mut self) -> Vec<u8> {
        self.flush();
        std::mem::take(&mut self.output)
    }

    pub(super) fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(super) fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// Whether frames or stream data still wait to be sent.
    pub(super) fn has_pending_data(&self) -> bool {
        !self.output.is_empty() || self.streams.values().any(|stream| !stream.queue.is_empty())
    }

    /// Whether `take_output` would yield bytes: queued frames, or DATA the windows allow.
    pub(super) fn wants_write(&self) -> bool {
        !self.terminated && (!self.output.is_empty() || self.streams.values().any(|stream| stream.sendable(self.send_window)))
    }

    pub(super) fn submit_settings(&mut self, entries: Vec<(u16, u32)>) -> Result<(), Refusal> {
        if self.sent_settings.len() >= MAX_OUTSTANDING {
            return Err(Refusal::TooManyPending);
        }
        frame::write(&mut self.output, Kind::Settings, 0, 0, &frame::settings_payload(&entries));
        self.sent_settings.push_back((entries, Instant::now()));
        Ok(())
    }

    pub(super) fn ping(&mut self, payload: [u8; frame::PING_LENGTH]) -> Result<(), Refusal> {
        if self.pings.len() >= MAX_OUTSTANDING {
            return Err(Refusal::TooManyPending);
        }
        frame::write(&mut self.output, Kind::Ping, 0, 0, &payload);
        self.pings.push_back(Instant::now());
        Ok(())
    }

    /// A GOAWAY naming `last_stream`, or the last stream the peer opened when it is zero.
    pub(super) fn goaway(&mut self, code: u32, last_stream: u32, data: &[u8]) {
        if self.terminated {
            return;
        }
        let last = if last_stream == 0 { self.last_peer_stream } else { last_stream };
        let mut payload = Vec::with_capacity(frame::GOAWAY_FIXED_LENGTH + data.len());
        payload.extend_from_slice(&(last & MAX_STREAM_ID).to_be_bytes());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(data);
        frame::write(&mut self.output, Kind::Goaway, 0, 0, &payload);
        self.goaway_sent = true;
    }

    /// End the connection with a final GOAWAY. Like nghttp2's terminating GOAWAY it is sent
    /// ahead of the frames still queued, which are then never sent.
    pub(super) fn terminate(&mut self, code: u32) {
        if self.terminated {
            return;
        }
        self.output.clear();
        self.goaway(code, 0, &[]);
        self.terminated = true;
    }

    pub(super) fn set_next_stream_id(&mut self, id: u32) {
        self.next_stream_id = id;
    }

    /// Grow or shrink the connection's receive window to `size` (`setLocalWindowSize`).
    pub(super) fn set_local_window_size(&mut self, size: i64) {
        let delta = size - self.recv_window;
        self.recv_window = size;
        if delta > 0 {
            self.window_update(0, delta);
        }
    }

    fn window_update(&mut self, stream: u32, increment: i64) {
        frame::write(&mut self.output, Kind::WindowUpdate, 0, stream, &(increment as u32).to_be_bytes());
    }

    fn insert_stream(&mut self, id: u32, stream: Stream) {
        self.streams.insert(id, stream);
        self.opened.push(id);
    }

    fn remove_stream(&mut self, id: u32) -> Option<Stream> {
        let stream = self.streams.remove(&id)?;
        self.opened.retain(|opened| *opened != id);
        Some(stream)
    }

    fn open_stream(&self) -> Stream {
        Stream { send_window: i64::from(self.remote.initial_window_size), ..Stream::default() }
    }

    fn allocate_stream_id(&mut self) -> Result<u32, Refusal> {
        let id = self.next_stream_id;
        if id > MAX_STREAM_ID {
            return Err(Refusal::OutOfStreams);
        }
        self.next_stream_id = id.saturating_add(STREAM_ID_STEP);
        Ok(id)
    }

    /// Open a stream with a request head (clients).
    pub(super) fn submit_request(&mut self, fields: &[Field], end_stream: bool, want_trailers: bool) -> Result<u32, Refusal> {
        let id = self.allocate_stream_id()?;
        let mut stream = self.open_stream();
        stream.want_trailers = want_trailers;
        self.insert_stream(id, stream);
        self.submit_headers(id, fields, end_stream)?;
        Ok(id)
    }

    /// A head (a response, informational head or request) on an open stream.
    pub(super) fn submit_headers(&mut self, id: u32, fields: &[Field], end_stream: bool) -> Result<(), Refusal> {
        let stream = self.streams.get_mut(&id).filter(|stream| !stream.local_closed).ok_or(Refusal::StreamClosed)?;
        stream.reserved_local = false;
        stream.head_sent |= !is_informational(fields);
        self.send_block(Kind::Headers, id, &[], fields, end_stream);
        if end_stream {
            self.close_local(id);
        }
        Ok(())
    }

    /// Mark a stream as awaiting trailers once its data drains (`waitForTrailers`).
    pub(super) fn want_trailers(&mut self, id: u32) {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.want_trailers = true;
        }
    }

    /// Trailers ending a stream; an empty list ends it with an empty DATA frame.
    pub(super) fn submit_trailers(&mut self, id: u32, fields: &[Field]) -> Result<(), Refusal> {
        let stream = self.streams.get_mut(&id).filter(|stream| !stream.local_closed).ok_or(Refusal::StreamClosed)?;
        stream.want_trailers = false;
        if fields.is_empty() {
            frame::write(&mut self.output, Kind::Data, flags::END_STREAM, id, &[]);
        } else {
            self.send_block(Kind::Headers, id, &[], fields, true);
        }
        self.close_local(id);
        Ok(())
    }

    /// Promise a pushed stream on `parent` (servers); the promised stream is reserved until
    /// its response head is submitted.
    pub(super) fn submit_push_promise(&mut self, parent: u32, fields: &[Field]) -> Result<u32, Refusal> {
        if !self.streams.get(&parent).is_some_and(|stream| !stream.local_closed) {
            return Err(Refusal::StreamClosed);
        }
        let id = self.allocate_stream_id()?;
        let mut stream = self.open_stream();
        stream.remote_closed = true;
        stream.reserved_local = true;
        self.insert_stream(id, stream);
        self.send_block(Kind::PushPromise, parent, &id.to_be_bytes(), fields, false);
        Ok(id)
    }

    fn send_block(&mut self, kind: Kind, stream: u32, prefix: &[u8], fields: &[Field], end_stream: bool) {
        let block = self.encoder.encode(fields);
        let limit = self.remote.max_frame_size as usize;
        let first_room = limit - prefix.len();
        let (first, mut rest) = block.split_at(block.len().min(first_room));
        let mut first_flags = if end_stream { flags::END_STREAM } else { 0 };
        if rest.is_empty() {
            first_flags |= flags::END_HEADERS;
        }
        let mut payload = prefix.to_vec();
        payload.extend_from_slice(first);
        frame::write(&mut self.output, kind, first_flags, stream, &payload);
        while !rest.is_empty() {
            let (part, remaining) = rest.split_at(rest.len().min(limit));
            rest = remaining;
            let continuation_flags = if rest.is_empty() { flags::END_HEADERS } else { 0 };
            frame::write(&mut self.output, Kind::Continuation, continuation_flags, stream, part);
        }
    }

    /// Queue `bytes` on a stream; a `WriteDone` follows once they are framed.
    pub(super) fn write(&mut self, id: u32, bytes: Vec<u8>) {
        match self.streams.get_mut(&id).filter(|stream| !stream.local_closed && !stream.end_requested) {
            Some(stream) => stream.queue.push_back(bytes),
            None => self.events.push(Event::WriteDone { stream: id }),
        }
    }

    /// End a stream's sending side once its queued data is framed.
    pub(super) fn shutdown(&mut self, id: u32) {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.end_requested = true;
        }
    }

    /// Reset a stream once what it queued is framed, as Node purges pending data before
    /// RST_STREAM; a stream that framing closes needs no reset.
    pub(super) fn rst_stream(&mut self, id: u32, code: u32) {
        self.flush();
        self.reset(id, code);
    }

    fn reset(&mut self, id: u32, code: u32) {
        if self.remove_stream(id).is_some() {
            frame::write(&mut self.output, Kind::RstStream, 0, id, &code.to_be_bytes());
            self.events.push(Event::StreamClosed { stream: id, code, peer_reset: false });
        }
    }

    pub(super) fn altsvc(&mut self, stream: u32, origin: &[u8], value: &[u8]) {
        let mut payload = (origin.len() as u16).to_be_bytes().to_vec();
        payload.extend_from_slice(origin);
        payload.extend_from_slice(value);
        frame::write(&mut self.output, Kind::AltSvc, 0, stream, &payload);
    }

    pub(super) fn origin(&mut self, origins: &[Vec<u8>]) {
        let mut payload = Vec::new();
        for origin in origins {
            payload.extend_from_slice(&(origin.len() as u16).to_be_bytes());
            payload.extend_from_slice(origin);
        }
        frame::write(&mut self.output, Kind::Origin, 0, 0, &payload);
    }

    fn close_local(&mut self, id: u32) {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.local_closed = true;
        }
        self.close_if_done(id);
    }

    fn close_if_done(&mut self, id: u32) {
        if self.streams.get(&id).is_some_and(|stream| stream.local_closed && stream.remote_closed) {
            self.remove_stream(id);
            self.events.push(Event::StreamClosed { stream: id, code: ErrorCode::NoError as u32, peer_reset: false });
        }
    }

    /// Frame queued data as the windows allow, ending streams whose shutdown was requested.
    fn flush(&mut self) {
        if self.terminated {
            return;
        }
        for id in self.opened.clone() {
            self.flush_stream(id);
        }
    }

    fn flush_stream(&mut self, id: u32) {
        let max_frame = i64::from(self.remote.max_frame_size);
        loop {
            let Some(stream) = self.streams.get_mut(&id) else { return };
            if stream.local_closed || !stream.head_sent {
                return;
            }
            let Some(chunk) = stream.queue.front() else {
                if !stream.end_requested {
                    return;
                }
                if stream.want_trailers {
                    if !stream.trailers_asked {
                        stream.trailers_asked = true;
                        self.events.push(Event::WantTrailers { stream: id });
                    }
                    return;
                }
                frame::write(&mut self.output, Kind::Data, flags::END_STREAM, id, &[]);
                self.close_local(id);
                return;
            };
            let remaining = chunk.len() - stream.sent_of_first;
            if remaining == 0 {
                stream.queue.pop_front();
                stream.sent_of_first = 0;
                self.events.push(Event::WriteDone { stream: id });
                continue;
            }
            let allowed = stream.send_window.min(self.send_window).min(max_frame);
            if allowed <= 0 {
                return;
            }
            let length = remaining.min(allowed as usize);
            let finishes_chunk = length == remaining;
            let last = finishes_chunk && stream.queue.len() == 1 && stream.end_requested && !stream.want_trailers;
            let start = stream.sent_of_first;
            let data_flags = if last { flags::END_STREAM } else { 0 };
            frame::write(&mut self.output, Kind::Data, data_flags, id, &chunk[start..start + length]);
            stream.send_window -= length as i64;
            self.send_window -= length as i64;
            if finishes_chunk {
                stream.queue.pop_front();
                stream.sent_of_first = 0;
                self.events.push(Event::WriteDone { stream: id });
            } else {
                stream.sent_of_first += length;
            }
            if last {
                self.close_local(id);
                return;
            }
        }
    }

    fn connection_error(&mut self, code: ErrorCode) {
        if self.terminated {
            return;
        }
        self.goaway(code as u32, 0, &[]);
        self.terminated = true;
        self.events.push(Event::ConnectionError { code: code as u32 });
    }

    /// Read `bytes` from the peer.
    pub(super) fn receive(&mut self, bytes: &[u8]) {
        if self.terminated {
            return;
        }
        self.input.extend_from_slice(bytes);
        let input = std::mem::take(&mut self.input);
        let mut at = 0;
        if self.awaiting_preface {
            let seen = input.len().min(frame::PREFACE.len());
            if input[..seen] != frame::PREFACE[..seen] {
                self.connection_error(ErrorCode::ProtocolError);
                return;
            }
            if seen < frame::PREFACE.len() {
                self.input = input;
                return;
            }
            self.awaiting_preface = false;
            at = frame::PREFACE.len();
        }
        while input.len() - at >= frame::HEADER_LENGTH && !self.terminated {
            let header = frame::Header::read(&input[at..]);
            if header.length > self.local.max_frame_size as usize {
                self.connection_error(ErrorCode::FrameSizeError);
                break;
            }
            let end = at + frame::HEADER_LENGTH + header.length;
            if end > input.len() {
                break;
            }
            self.frame(&header, &input[at + frame::HEADER_LENGTH..end]);
            at = end;
        }
        if !self.terminated {
            self.input = input[at..].to_vec();
        }
    }

    fn frame(&mut self, header: &frame::Header, payload: &[u8]) {
        let kind = Kind::from_wire(header.kind);
        if let Some(block) = &self.block {
            if kind != Some(Kind::Continuation) || header.stream != block.stream {
                self.connection_error(ErrorCode::ProtocolError);
                return;
            }
        }
        let connection_frame = matches!(kind, Some(Kind::Settings | Kind::Ping | Kind::Goaway));
        let stream_frame = matches!(
            kind,
            Some(Kind::Data | Kind::Headers | Kind::Priority | Kind::RstStream | Kind::PushPromise | Kind::Continuation)
        );
        if (connection_frame && header.stream != 0) || (stream_frame && header.stream == 0) {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        }
        match kind {
            Some(Kind::Data) => self.data(header, payload),
            Some(Kind::Headers) => self.headers(header, payload),
            Some(Kind::Priority) => {
                if payload.len() != frame::PRIORITY_LENGTH {
                    self.connection_error(ErrorCode::FrameSizeError);
                }
            }
            Some(Kind::RstStream) => {
                if payload.len() != frame::RST_STREAM_LENGTH {
                    self.connection_error(ErrorCode::FrameSizeError);
                    return;
                }
                if self.remove_stream(header.stream).is_some() {
                    self.events.push(Event::StreamClosed { stream: header.stream, code: frame::read_u32(payload), peer_reset: true });
                }
            }
            Some(Kind::Settings) => self.settings(header, payload),
            Some(Kind::PushPromise) => self.push_promise(header, payload),
            Some(Kind::Ping) => self.ping_frame(header, payload),
            Some(Kind::Goaway) => self.goaway_frame(payload),
            Some(Kind::WindowUpdate) => self.window_update_frame(header, payload),
            Some(Kind::Continuation) => self.continuation(header, payload),
            Some(Kind::AltSvc) => self.altsvc_frame(header, payload),
            Some(Kind::Origin) => self.origin_frame(header, payload),
            None => {}
        }
    }

    /// Whether `id` names a stream the peer may still open or that we opened and closed.
    fn idle(&self, id: u32) -> bool {
        if initiator(id) != self.role { id > self.last_peer_stream } else { id >= self.next_stream_id }
    }

    fn data(&mut self, header: &frame::Header, payload: &[u8]) {
        let Some((start, end)) = unpadded(header, payload) else {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        };
        let length = payload.len() as i64;
        self.recv_unacked += length;
        if self.recv_unacked >= self.recv_window / WINDOW_UPDATE_DIVISOR {
            let increment = std::mem::take(&mut self.recv_unacked);
            self.window_update(0, increment);
        }
        let id = header.stream;
        let threshold = i64::from(self.local.initial_window_size) / WINDOW_UPDATE_DIVISOR;
        let Some(stream) = self.streams.get_mut(&id) else {
            if self.idle(id) {
                self.connection_error(ErrorCode::ProtocolError);
            } else {
                frame::write(&mut self.output, Kind::RstStream, 0, id, &(ErrorCode::StreamClosed as u32).to_be_bytes());
            }
            return;
        };
        if stream.remote_closed {
            self.reset(id, ErrorCode::StreamClosed as u32);
            return;
        }
        let end_stream = header.flags & flags::END_STREAM != 0;
        stream.recv_unacked += length;
        let replenish = (!end_stream && stream.recv_unacked >= threshold).then(|| std::mem::take(&mut stream.recv_unacked));
        if end_stream {
            stream.remote_closed = true;
        }
        if let Some(increment) = replenish.filter(|&increment| increment > 0) {
            self.window_update(id, increment);
        }
        self.events.push(Event::Data { stream: id, bytes: payload[start..end].to_vec(), end: end_stream });
        if end_stream {
            self.close_if_done(id);
        }
    }

    fn headers(&mut self, header: &frame::Header, payload: &[u8]) {
        let Some((mut start, end)) = unpadded(header, payload) else {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        };
        if header.flags & flags::PRIORITY != 0 {
            start += frame::PRIORITY_LENGTH;
            if start > end {
                self.connection_error(ErrorCode::FrameSizeError);
                return;
            }
        }
        self.begin_block(header, None, &payload[start..end]);
    }

    fn push_promise(&mut self, header: &frame::Header, payload: &[u8]) {
        if self.role == Role::Server || !self.local.enable_push {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        }
        let Some((start, end)) = unpadded(header, payload).filter(|(start, end)| end - start >= frame::PUSH_PROMISE_ID_LENGTH) else {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        };
        let promised = frame::read_u32(&payload[start..]) & MAX_STREAM_ID;
        self.begin_block(header, Some(promised), &payload[start + frame::PUSH_PROMISE_ID_LENGTH..end]);
    }

    fn begin_block(&mut self, header: &frame::Header, promised: Option<u32>, fragment: &[u8]) {
        let block = Block { stream: header.stream, promised, flags: header.flags, fragment: fragment.to_vec() };
        if header.flags & flags::END_HEADERS != 0 {
            self.complete_block(block);
        } else {
            self.block = Some(block);
        }
    }

    fn continuation(&mut self, header: &frame::Header, payload: &[u8]) {
        let Some(mut block) = self.block.take() else {
            self.connection_error(ErrorCode::ProtocolError);
            return;
        };
        block.fragment.extend_from_slice(payload);
        if header.flags & flags::END_HEADERS != 0 {
            self.complete_block(block);
        } else {
            self.block = Some(block);
        }
    }

    fn complete_block(&mut self, block: Block) {
        let Ok(fields) = self.decoder.decode(&block.fragment) else {
            self.connection_error(ErrorCode::CompressionError);
            return;
        };
        let event_flags = (block.flags & (flags::END_STREAM | flags::PRIORITY)) | flags::END_HEADERS;
        if let Some(promised) = block.promised {
            if !self.streams.contains_key(&block.stream) || !server_initiated(promised) || !self.idle(promised) {
                self.connection_error(ErrorCode::ProtocolError);
                return;
            }
            self.last_peer_stream = promised;
            let mut stream = self.open_stream();
            stream.local_closed = true;
            stream.promised = true;
            self.insert_stream(promised, stream);
            let flags = flags::END_HEADERS;
            self.events.push(Event::Headers { stream: promised, category: Category::Request, flags, fields });
            return;
        }
        let id = block.stream;
        let end_stream = block.flags & flags::END_STREAM != 0;
        let category = match self.streams.get_mut(&id) {
            Some(stream) if stream.remote_closed => {
                self.reset(id, ErrorCode::StreamClosed as u32);
                return;
            }
            Some(stream) if self.role == Role::Client && !stream.final_head => {
                let informational = !end_stream && is_informational(&fields);
                stream.final_head = !informational;
                if std::mem::take(&mut stream.promised) { Category::PushResponse } else { Category::Response }
            }
            Some(_) => Category::Headers,
            None => {
                if self.role == Role::Client || !self.idle(id) {
                    self.connection_error(ErrorCode::ProtocolError);
                    return;
                }
                self.last_peer_stream = id;
                if self.goaway_sent {
                    frame::write(&mut self.output, Kind::RstStream, 0, id, &(ErrorCode::RefusedStream as u32).to_be_bytes());
                    return;
                }
                let stream = self.open_stream();
                self.insert_stream(id, stream);
                Category::Request
            }
        };
        if end_stream {
            if let Some(stream) = self.streams.get_mut(&id) {
                stream.remote_closed = true;
            }
        }
        self.events.push(Event::Headers { stream: id, category, flags: event_flags, fields });
        if end_stream {
            self.close_if_done(id);
        }
    }

    fn settings(&mut self, header: &frame::Header, payload: &[u8]) {
        if header.flags & flags::ACK != 0 {
            if !payload.is_empty() {
                self.connection_error(ErrorCode::FrameSizeError);
                return;
            }
            let Some((entries, sent)) = self.sent_settings.pop_front() else {
                self.connection_error(ErrorCode::ProtocolError);
                return;
            };
            self.local.apply(&entries);
            self.decoder.set_limit(self.local.header_table_size as usize);
            self.events.push(Event::SettingsAck { duration: milliseconds_since(sent) });
            return;
        }
        if !payload.len().is_multiple_of(frame::SETTING_LENGTH) {
            self.connection_error(ErrorCode::FrameSizeError);
            return;
        }
        let entries: Vec<(u16, u32)> = frame::settings_entries(payload).collect();
        for &(id, value) in &entries {
            let invalid = match id {
                id if id == SettingId::EnablePush as u16 => value > 1 || (self.role == Role::Client && value == 1),
                id if id == SettingId::InitialWindowSize as u16 => i64::from(value) > frame::MAX_WINDOW,
                id if id == SettingId::MaxFrameSize as u16 => {
                    !(frame::MIN_MAX_FRAME_SIZE..=frame::MAX_MAX_FRAME_SIZE).contains(&value)
                }
                id if id == SettingId::EnableConnectProtocol as u16 => value > 1,
                _ => false,
            };
            if invalid {
                let code = if id == SettingId::InitialWindowSize as u16 {
                    ErrorCode::FlowControlError
                } else {
                    ErrorCode::ProtocolError
                };
                self.connection_error(code);
                return;
            }
        }
        if !std::mem::replace(&mut self.remote_settings_received, true) {
            self.remote.max_concurrent_streams = Settings::INITIAL.max_concurrent_streams;
        }
        let previous_window = i64::from(self.remote.initial_window_size);
        self.remote.apply(&entries);
        let delta = i64::from(self.remote.initial_window_size) - previous_window;
        for stream in self.streams.values_mut() {
            stream.send_window += delta;
        }
        self.encoder.set_peer_limit(self.remote.header_table_size as usize);
        frame::write(&mut self.output, Kind::Settings, flags::ACK, 0, &[]);
        self.events.push(Event::RemoteSettings);
    }

    fn ping_frame(&mut self, header: &frame::Header, payload: &[u8]) {
        let Ok(payload) = <[u8; frame::PING_LENGTH]>::try_from(payload) else {
            self.connection_error(ErrorCode::FrameSizeError);
            return;
        };
        if header.flags & flags::ACK == 0 {
            frame::write(&mut self.output, Kind::Ping, flags::ACK, 0, &payload);
            self.events.push(Event::Ping { payload });
        } else if let Some(sent) = self.pings.pop_front() {
            self.events.push(Event::PingAck { payload, duration: milliseconds_since(sent) });
        }
    }

    fn goaway_frame(&mut self, payload: &[u8]) {
        if payload.len() < frame::GOAWAY_FIXED_LENGTH {
            self.connection_error(ErrorCode::FrameSizeError);
            return;
        }
        let last_stream = frame::read_u32(payload) & MAX_STREAM_ID;
        let code = frame::read_u32(&payload[frame::GOAWAY_LAST_STREAM_LENGTH..]);
        self.events.push(Event::Goaway { code, last_stream, data: payload[frame::GOAWAY_FIXED_LENGTH..].to_vec() });
        let refused: Vec<u32> =
            self.opened.iter().copied().filter(|&id| id > last_stream && initiator(id) == self.role).collect();
        for id in refused {
            self.remove_stream(id);
            self.events.push(Event::StreamClosed { stream: id, code: ErrorCode::RefusedStream as u32, peer_reset: false });
        }
    }

    fn window_update_frame(&mut self, header: &frame::Header, payload: &[u8]) {
        if payload.len() != frame::WINDOW_UPDATE_LENGTH {
            self.connection_error(ErrorCode::FrameSizeError);
            return;
        }
        let increment = i64::from(frame::read_u32(payload) & MAX_STREAM_ID);
        if header.stream == 0 {
            if increment == 0 {
                self.connection_error(ErrorCode::ProtocolError);
                return;
            }
            self.send_window += increment;
            if self.send_window > frame::MAX_WINDOW {
                self.connection_error(ErrorCode::FlowControlError);
            }
            return;
        }
        let Some(stream) = self.streams.get_mut(&header.stream) else { return };
        if increment == 0 {
            self.reset(header.stream, ErrorCode::ProtocolError as u32);
            return;
        }
        stream.send_window += increment;
        if stream.send_window > frame::MAX_WINDOW {
            self.reset(header.stream, ErrorCode::FlowControlError as u32);
        }
    }

    fn altsvc_frame(&mut self, header: &frame::Header, payload: &[u8]) {
        if self.role == Role::Server || payload.len() < frame::ALTSVC_ORIGIN_LENGTH {
            return;
        }
        let origin_length = usize::from(frame::read_u16(payload));
        let Some(value) = payload.get(frame::ALTSVC_ORIGIN_LENGTH + origin_length..) else { return };
        let origin = payload[frame::ALTSVC_ORIGIN_LENGTH..frame::ALTSVC_ORIGIN_LENGTH + origin_length].to_vec();
        self.events.push(Event::AltSvc { stream: header.stream, origin, value: value.to_vec() });
    }

    fn origin_frame(&mut self, header: &frame::Header, payload: &[u8]) {
        if self.role == Role::Server || header.stream != 0 {
            return;
        }
        let mut origins = Vec::new();
        let mut rest = payload;
        while rest.len() >= frame::ORIGIN_ENTRY_LENGTH {
            let length = usize::from(frame::read_u16(rest));
            let Some(origin) = rest.get(frame::ORIGIN_ENTRY_LENGTH..frame::ORIGIN_ENTRY_LENGTH + length) else { return };
            origins.push(origin.to_vec());
            rest = &rest[frame::ORIGIN_ENTRY_LENGTH + length..];
        }
        self.events.push(Event::Origin { origins });
    }

    pub(super) fn session_state(&self) -> SessionState {
        SessionState {
            effective_local_window_size: self.recv_window as f64,
            effective_recv_data_length: self.recv_unacked as f64,
            next_stream_id: f64::from(self.next_stream_id),
            local_window_size: (self.recv_window - self.recv_unacked) as f64,
            last_proc_stream_id: f64::from(self.last_peer_stream),
            remote_window_size: self.send_window as f64,
            outbound_queue_size: self.streams.values().map(|stream| stream.queue.len()).sum::<usize>() as f64,
            deflate_dynamic_table_size: self.encoder.table_size() as f64,
            inflate_dynamic_table_size: self.decoder.table_size() as f64,
        }
    }

    pub(super) fn stream_state(&self, id: u32) -> StreamState {
        let Some(stream) = self.streams.get(&id) else {
            return StreamState { phase: StreamPhase::Closed, local_close: true, remote_close: true, local_window_size: 0.0 };
        };
        let phase = match (stream.reserved_local, stream.promised, stream.local_closed, stream.remote_closed) {
            (true, _, _, _) => StreamPhase::ReservedLocal,
            (_, true, _, _) => StreamPhase::ReservedRemote,
            (_, _, true, _) => StreamPhase::HalfClosedLocal,
            (_, _, _, true) => StreamPhase::HalfClosedRemote,
            _ => StreamPhase::Open,
        };
        StreamState {
            phase,
            local_close: stream.local_closed,
            remote_close: stream.remote_closed,
            local_window_size: (i64::from(self.local.initial_window_size) - stream.recv_unacked) as f64,
        }
    }
}

/// The endpoint that opens stream `id`: clients odd ids, servers even (RFC 9113 §5.1.1).
fn initiator(id: u32) -> Role {
    if id % STREAM_ID_STEP == SERVER_FIRST_STREAM % STREAM_ID_STEP { Role::Server } else { Role::Client }
}

/// Whether stream `id` was opened by a server: a pushed stream.
pub(super) fn server_initiated(id: u32) -> bool {
    initiator(id) == Role::Server
}

/// Whether a head's `:status` is informational (1xx), as a response that is not final.
fn is_informational(fields: &[Field]) -> bool {
    const INFORMATIONAL: u8 = b'1';
    use crate::modules::http_shared_vm::parser::STATUS_DIGITS;
    fields
        .iter()
        .find(|field| field.name == b":status")
        .is_some_and(|field| field.value.len() == STATUS_DIGITS && field.value[0] == INFORMATIONAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, value: &str) -> Field {
        Field { name: name.into(), value: value.into(), sensitive: false }
    }

    /// Carry each side's output to the other until both are quiet.
    fn exchange(client: &mut Connection, server: &mut Connection) -> (Vec<Event>, Vec<Event>) {
        let (mut client_events, mut server_events) = (Vec::new(), Vec::new());
        loop {
            let to_server = client.take_output();
            let to_client = server.take_output();
            if to_server.is_empty() && to_client.is_empty() {
                break;
            }
            server.receive(&to_server);
            client.receive(&to_client);
            client_events.extend(client.take_events());
            server_events.extend(server.take_events());
        }
        (client_events, server_events)
    }

    #[test]
    fn request_and_response_close_the_stream() {
        let mut client = Connection::new(Role::Client);
        let mut server = Connection::new(Role::Server);
        client.submit_settings(Vec::new()).unwrap();
        server.submit_settings(Vec::new()).unwrap();
        let id = client.submit_request(&[field(":method", "POST"), field(":path", "/")], false, false).unwrap();
        client.write(id, vec![1; 100_000]);
        client.shutdown(id);
        let (_, server_events) = exchange(&mut client, &mut server);
        let received: usize = server_events
            .iter()
            .map(|event| if let Event::Data { bytes, .. } = event { bytes.len() } else { 0 })
            .sum();
        assert_eq!(received, 100_000);
        assert!(server_events.iter().any(|event| matches!(event, Event::Data { end: true, .. })));
        server.submit_headers(id, &[field(":status", "200")], false).unwrap();
        server.write(id, b"hello".to_vec());
        server.shutdown(id);
        let (client_events, server_events) = exchange(&mut client, &mut server);
        assert!(client_events.iter().any(|event| matches!(event, Event::Headers { category: Category::Response, .. })));
        assert!(client_events.iter().any(|event| matches!(event, Event::StreamClosed { code: 0, .. })));
        assert!(server_events.iter().any(|event| matches!(event, Event::StreamClosed { code: 0, .. })));
    }

    #[test]
    fn rejects_a_bad_preface() {
        let mut server = Connection::new(Role::Server);
        server.receive(b"GET / HTTP/1.1\r\n");
        assert!(matches!(server.take_events()[..], [Event::ConnectionError { .. }]));
    }
}
