# Advanced green Node API coverage

Target: http2, sqlite, trace_events, quic. Require real observable loopback/backend behavior, not empty namespaces or silent fallbacks. For each module: upstream API inventory, focused fixture, resolver/dispatch wiring, lint-compliant Rust, run-compat and applicable tests/node evidence.

Evidence (2026-08-21): `test-http2.js`, `test-sqlite.js`, and `test-trace-events.js` each pass with `cargo run -p quench-node-test --bin run-compat -- --quiet --filter {http2,sqlite,trace-events}`. `test-quic.js` exercises real UDP bind/send/close through `node:quic` and passes with `--filter quic`. QUIC currently documents a UDP loopback subset rather than claiming TLS/streams/congestion-control parity.

Verification: cargo build/test, run-compat, run-parallel, tools/lint-rust.sh.