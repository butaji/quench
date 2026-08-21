# Advanced green Node API coverage

Target: Bun green modules http2, sqlite, trace_events, and quic. Require real
observable loopback/backend behavior, not empty namespaces or silent fallbacks.
For each module: upstream API inventory, focused fixture, resolver/dispatch
wiring, lint-compliant Rust, run-compat, and applicable Node API evidence.

Bun caveats MUST be recorded: http2 has documented ignored options and a
partial upstream suite; sqlite backup blocks and has platform/path caveats;
trace categories are reduced; quic is experimental. Quench's current quic
fixture demonstrates a UDP loopback subset only and MUST NOT be described as
full QUIC/TLS/stream/congestion-control parity.

Current measured evidence: the focused suite passes 57/57 and the upstream
parallel manifest passes 178/178. QUIC remains a focused UDP loopback subset,
not full Bun-level QUIC parity. Any broader advanced-module claim requires
related Node API tests and recorded results.