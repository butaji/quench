# Verification and delivery

Current verified evidence:
- `cargo test --workspace`: 76 tests passed.
- `cargo run -p quench-node-test --bin run-compat -- --quiet`: 57 fixtures passed, 0 failed.
- `cargo run -p quench-node-test --bin run-parallel`: 178 upstream parallel fixtures passed, 0 failed.
- Express, Koa, and Fastify focused smoke fixtures pass.
- `node:dgram` (metadata/TTL/membership/multicast/queue/ref), `node:net` (getConnections/ref/unref/byte counters), `node:sqlite`, `node:http2`, `node:quic`, crypto vectors (SHA-1/256/384/512 + real MD5/SHA-512 + crypto.subtle.digest/importKey), `node:child_process` exec/exec/execFileSync, `node:diagnostics_channel`, `node:dns` (callback + promises), worker isolation, `node:test` async/subtest/todo, and web-global fixtures (Event/CustomEvent/BroadcastChannel/web streams + text/compression stream variants) have focused evidence.
- New globals installed in host: Event, CustomEvent (Event-inherited), BroadcastChannel, ReadableStream/WritableStream/TransformStream/TextDecoderStream/TextEncoderStream/CompressionStream/DecompressionStream, crypto (with subtle).
- Upstream submodules remain untouched.

Required final gate: `tools/lint-rust.sh` with file <=500 lines, function <=40 lines, cognitive complexity <=10. Current baseline is still red; each lint task must split only owned files, preserve semantics, and update this evidence after verification. Never mark this plan complete while any gate is red.