# Verification and delivery

Current verified evidence:
- `cargo test --workspace`: 76 tests passed.
- `cargo run -p quench-node-test --bin run-compat -- --quiet`: 56 fixtures passed, 0 failed.
- `cargo run -p quench-node-test --bin run-parallel`: 178 upstream parallel fixtures passed, 0 failed.
- Express, Koa, and Fastify focused smoke fixtures pass.
- `node:dgram`, `node:sqlite`, `node:http2`, `node:quic`, crypto vectors, worker isolation, and web-global fixtures have focused evidence.
- Upstream submodules remain untouched.

Required final gate: `tools/lint-rust.sh` with file <=500 lines, function <=40 lines, cognitive complexity <=10. Current baseline is still red; each lint task must split only owned files, preserve semantics, and update this evidence after verification. Never mark this plan complete while any gate is red.