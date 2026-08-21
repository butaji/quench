# Advanced green Node API coverage

Target: http2, sqlite, trace_events, quic. Require real observable loopback/backend behavior, not empty namespaces or silent fallbacks. For each module: upstream API inventory, focused fixture, resolver/dispatch wiring, lint-compliant Rust, run-compat and applicable tests/node evidence.

Verification: cargo build/test, run-compat, run-parallel, tools/lint-rust.sh.