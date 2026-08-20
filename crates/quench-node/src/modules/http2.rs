//! Minimal `node:http2` compatibility adapter.
//!
//! The runtime's socket engine currently speaks HTTP/1.1.  For the loopback
//! subset we expose the familiar server surface and route it through that
//! engine; this is deliberately not advertised as a QUIC or TLS backend.
use quench_runtime::value::Value;

/// Build the supported loopback HTTP/2-compatible namespace.
///
/// `createServer` uses the same request/response event contract as `http`,
/// which is sufficient for local protocol fixtures and keeps transport
/// ownership in the existing, tested net implementation.
pub fn build() -> Value {
    let http = crate::modules::http::build();
    let create_server = quench_runtime::execute::get_property(&http, "createServer");
    let request = quench_runtime::execute::get_property(&http, "request");
    let get = quench_runtime::execute::get_property(&http, "get");
    let constants = quench_runtime::host_api::object(vec![
        (
            "HTTP2_HEADER_STATUS".to_string(),
            Value::String(":status".into()),
        ),
        (
            "HTTP2_HEADER_METHOD".to_string(),
            Value::String(":method".into()),
        ),
        (
            "HTTP2_HEADER_PATH".to_string(),
            Value::String(":path".into()),
        ),
        (
            "HTTP2_HEADER_SCHEME".to_string(),
            Value::String(":scheme".into()),
        ),
        (
            "HTTP2_HEADER_AUTHORITY".to_string(),
            Value::String(":authority".into()),
        ),
    ]);
    crate::host::namespace_object(vec![
        ("createServer", create_server),
        ("request", request),
        ("get", get),
        ("constants", constants),
    ])
    .unwrap_or(Value::Undefined)
}
