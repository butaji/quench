//! The errors `http2` raises (lib/internal/errors.js's `ERR_HTTP2_*` and the generic ones
//! its validators share), each as its code, class and message template.

use crate::modules::http_shared_vm::{coded, ErrorClass};
use crate::modules::stream_shared_vm::{Context, Outcome};
use quench_runtime::RootedError;

#[derive(Clone, Copy)]
pub(super) enum Fault {
    AltsvcInvalidOrigin,
    AltsvcLength,
    ConnectAuthority,
    ConnectPath,
    ConnectScheme,
    GoawaySession,
    HeadersAfterRespond,
    HeadersSent,
    HeaderSingleValue,
    InfoStatusNotAllowed,
    InvalidConnectionHeaders,
    InvalidHeaderValue,
    InvalidInfoStatus,
    InvalidOrigin,
    InvalidPackedSettingsLength,
    InvalidPseudoheader,
    InvalidSession,
    InvalidSettingValue,
    InvalidSettingRange,
    InvalidStream,
    MaxPendingSettingsAck,
    NestedPush,
    NoSocketManipulation,
    OriginLength,
    OutOfStreams,
    PayloadForbidden,
    PingCancel,
    PingLength,
    PseudoheaderNotAllowed,
    PushDisabled,
    SendFile,
    SendFileNoSeek,
    SessionError,
    SettingsCancel,
    SocketBound,
    SocketUnbound,
    Status101,
    StatusInvalid,
    StreamCancel,
    StreamError,
    TooManyCustomSettings,
    TrailersAlreadySent,
    TrailersNotReady,
    UnsupportedProtocol,
    StreamWriteAfterEnd,
    SocketClosed,
    /// nghttp2's own failures (`NghttpError`).
    Nghttp,
}

impl Fault {
    fn spec(self) -> (&'static str, ErrorClass, &'static str) {
        use ErrorClass::{Error, RangeError, TypeError};
        match self {
            Self::AltsvcInvalidOrigin => {
                ("ERR_HTTP2_ALTSVC_INVALID_ORIGIN", TypeError, "HTTP/2 ALTSVC frames require a valid origin")
            }
            Self::AltsvcLength => ("ERR_HTTP2_ALTSVC_LENGTH", TypeError, "HTTP/2 ALTSVC frames are limited to 16382 bytes"),
            Self::ConnectAuthority => {
                ("ERR_HTTP2_CONNECT_AUTHORITY", Error, ":authority header is required for CONNECT requests")
            }
            Self::ConnectPath => ("ERR_HTTP2_CONNECT_PATH", Error, "The :path header is forbidden for CONNECT requests"),
            Self::ConnectScheme => {
                ("ERR_HTTP2_CONNECT_SCHEME", Error, "The :scheme header is forbidden for CONNECT requests")
            }
            Self::GoawaySession => {
                ("ERR_HTTP2_GOAWAY_SESSION", Error, "New streams cannot be created after receiving a GOAWAY")
            }
            Self::HeadersAfterRespond => {
                ("ERR_HTTP2_HEADERS_AFTER_RESPOND", Error, "Cannot specify additional headers after response initiated")
            }
            Self::HeadersSent => ("ERR_HTTP2_HEADERS_SENT", Error, "Response has already been initiated."),
            Self::HeaderSingleValue => {
                ("ERR_HTTP2_HEADER_SINGLE_VALUE", TypeError, "Header field \"%s\" must only have a single value")
            }
            Self::InfoStatusNotAllowed => {
                ("ERR_HTTP2_INFO_STATUS_NOT_ALLOWED", RangeError, "Informational status codes cannot be used")
            }
            Self::InvalidConnectionHeaders => (
                "ERR_HTTP2_INVALID_CONNECTION_HEADERS",
                TypeError,
                "HTTP/1 Connection specific headers are forbidden: \"%s\"",
            ),
            Self::InvalidHeaderValue => ("ERR_HTTP2_INVALID_HEADER_VALUE", TypeError, "Invalid value \"%s\" for header \"%s\""),
            Self::InvalidInfoStatus => ("ERR_HTTP2_INVALID_INFO_STATUS", RangeError, "Invalid informational status code: %s"),
            Self::InvalidOrigin => ("ERR_HTTP2_INVALID_ORIGIN", TypeError, "HTTP/2 ORIGIN frames require a valid origin"),
            Self::InvalidPackedSettingsLength => (
                "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH",
                RangeError,
                "Packed settings length must be a multiple of six",
            ),
            Self::InvalidPseudoheader => {
                ("ERR_HTTP2_INVALID_PSEUDOHEADER", TypeError, "\"%s\" is an invalid pseudoheader or is used incorrectly")
            }
            Self::InvalidSession => ("ERR_HTTP2_INVALID_SESSION", Error, "The session has been destroyed"),
            Self::InvalidSettingValue => {
                ("ERR_HTTP2_INVALID_SETTING_VALUE", TypeError, "Invalid value for setting \"%s\": %s")
            }
            Self::InvalidSettingRange => {
                ("ERR_HTTP2_INVALID_SETTING_VALUE", RangeError, "Invalid value for setting \"%s\": %s")
            }
            Self::InvalidStream => ("ERR_HTTP2_INVALID_STREAM", Error, "The stream has been destroyed"),
            Self::MaxPendingSettingsAck => {
                ("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK", Error, "Maximum number of pending settings acknowledgements")
            }
            Self::NestedPush => ("ERR_HTTP2_NESTED_PUSH", Error, "A push stream cannot initiate another push stream."),
            Self::NoSocketManipulation => (
                "ERR_HTTP2_NO_SOCKET_MANIPULATION",
                Error,
                "HTTP/2 sockets should not be directly manipulated (e.g. read and written)",
            ),
            Self::OriginLength => ("ERR_HTTP2_ORIGIN_LENGTH", TypeError, "HTTP/2 ORIGIN frames are limited to 16382 bytes"),
            Self::OutOfStreams => (
                "ERR_HTTP2_OUT_OF_STREAMS",
                Error,
                "No stream ID is available because maximum stream ID has been reached",
            ),
            Self::PayloadForbidden => ("ERR_HTTP2_PAYLOAD_FORBIDDEN", Error, "Responses with %s status must not have a payload"),
            Self::PingCancel => ("ERR_HTTP2_PING_CANCEL", Error, "HTTP2 ping cancelled"),
            Self::PingLength => ("ERR_HTTP2_PING_LENGTH", RangeError, "HTTP2 ping payload must be 8 bytes"),
            Self::PseudoheaderNotAllowed => ("ERR_HTTP2_PSEUDOHEADER_NOT_ALLOWED", TypeError, "Cannot set HTTP/2 pseudo-headers"),
            Self::PushDisabled => ("ERR_HTTP2_PUSH_DISABLED", Error, "HTTP/2 client has disabled push streams"),
            Self::SendFile => ("ERR_HTTP2_SEND_FILE", Error, "Directories cannot be sent"),
            Self::SendFileNoSeek => {
                ("ERR_HTTP2_SEND_FILE_NOSEEK", Error, "Offset or length can only be specified for regular files")
            }
            Self::SessionError => ("ERR_HTTP2_SESSION_ERROR", Error, "Session closed with error code %s"),
            Self::SettingsCancel => ("ERR_HTTP2_SETTINGS_CANCEL", Error, "HTTP2 session settings canceled"),
            Self::SocketBound => ("ERR_HTTP2_SOCKET_BOUND", Error, "The socket is already bound to an Http2Session"),
            Self::SocketUnbound => {
                ("ERR_HTTP2_SOCKET_UNBOUND", Error, "The socket has been disconnected from the Http2Session")
            }
            Self::Status101 => {
                ("ERR_HTTP2_STATUS_101", Error, "HTTP status code 101 (Switching Protocols) is forbidden in HTTP/2")
            }
            Self::StatusInvalid => ("ERR_HTTP2_STATUS_INVALID", RangeError, "Invalid status code: %s"),
            Self::StreamCancel => ("ERR_HTTP2_STREAM_CANCEL", Error, "The pending stream has been canceled%s"),
            Self::StreamError => ("ERR_HTTP2_STREAM_ERROR", Error, "Stream closed with error code %s"),
            Self::TooManyCustomSettings => (
                "ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS",
                Error,
                "Number of custom settings exceeds MAX_ADDITIONAL_SETTINGS",
            ),
            Self::TrailersAlreadySent => ("ERR_HTTP2_TRAILERS_ALREADY_SENT", Error, "Trailing headers have already been sent"),
            Self::TrailersNotReady => (
                "ERR_HTTP2_TRAILERS_NOT_READY",
                Error,
                "Trailing headers cannot be sent until after the wantTrailers event is emitted",
            ),
            Self::UnsupportedProtocol => ("ERR_HTTP2_UNSUPPORTED_PROTOCOL", Error, "protocol \"%s\" is unsupported."),
            Self::StreamWriteAfterEnd => ("ERR_STREAM_WRITE_AFTER_END", Error, "write after end"),
            Self::SocketClosed => ("ERR_SOCKET_CLOSED", Error, "Socket is closed"),
            Self::Nghttp => ("ERR_HTTP2_ERROR", Error, "%s"),
        }
    }

    /// The error, its template's `%s` placeholders filled by `values` in order.
    pub(super) fn error(self, c: &mut Context<'_>, values: &[&str]) -> Outcome {
        let (code, class, template) = self.spec();
        let mut message = String::with_capacity(template.len());
        let mut values = values.iter();
        let mut parts = template.split("%s");
        message.push_str(parts.next().unwrap_or_default());
        for part in parts {
            message.push_str(values.next().copied().unwrap_or_default());
            message.push_str(part);
        }
        coded(c, class, code, &message)
    }

    pub(super) fn throw(self, c: &mut Context<'_>, values: &[&str]) -> RootedError {
        match self.error(c, values) {
            Ok(error) => c.throw(error),
            Err(error) => error,
        }
    }
}

/// nghttp2's error numbers this implementation reports, with nghttp2's messages.
#[derive(Clone, Copy)]
pub(super) enum NghttpCode {
    Protocol = -505,
    BadClientMagic = -903,
}

impl NghttpCode {
    fn message(self) -> &'static str {
        match self {
            Self::Protocol => "Protocol error",
            Self::BadClientMagic => "Received bad client magic byte string",
        }
    }

    /// Node's `NghttpError`: the message, `code` ERR_HTTP2_ERROR and `errno`.
    pub(super) fn error(self, c: &mut Context<'_>) -> Outcome {
        let error = Fault::Nghttp.error(c, &[self.message()])?;
        let errno = c.number(f64::from(self as i32));
        c.assign_named_property_rooted(error, "errno", errno)?;
        Ok(error)
    }
}
