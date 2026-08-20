//! Minimal `node:quic` protocol adapter.
//!
//! QUIC is intentionally exposed as a loopback-capable datagram transport until
//! a QUIC packet engine is available. The adapter uses the real nonblocking UDP
//! sockets from `dgram`, preserving observable bind/send/message/close behavior
//! without pretending to implement TLS, streams, or QUIC congestion control.

use quench_runtime::host_api;
use quench_runtime::value::Value;

/// Build the transport namespace. `createSocket` returns the same real UDP
/// socket used by `node:dgram`; callers can bind to port 0 and use `address()`
/// to discover an ephemeral loopback endpoint.
pub fn build() -> Value {
    host_api::object(vec![(
        "createSocket".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("quic:createSocket", 0x2300)),
    )])
}
