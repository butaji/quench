# JetStream 3 no-JIT sweep, 2026-10-17

quench release build of `v2-cloud-jet` at ea848f09 (target-exp, `cargo build --release -p quench-node`)
against Bun 1.x with `BUN_JSC_useJIT=0` and Node v26.10.0 `--jitless`. Each test ran once,
sequentially, through `js3-prelude.js` (JetStream shell surface on `node:vm`), default
iteration counts, 900 s timeout.

Host: Linux 6.18.44-fc-v139 x86_64, 4 CPUs, 15 GiB RAM (cloud container).
The Bun and Node scores were measured earlier in this session on the same container type and are
reused here (their binaries did not change); `prior_quench` is the quench score from that earlier
sweep. Single runs: per-test noise is a few percent.

See `summary.txt` (geomeans and the per-test table, worst ratio to Bun first) and `rows.jsonl`.
