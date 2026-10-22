# JetStream 3 no-JIT, canonical runner, 2026-10-19

`target/production/quench-bench --all --suite jetstream3 --runs 3 --timeout-ms 600000`
with `target/production/quench-node` built from `v2-cloud-jet` at 98c39a5e
(`cargo build --locked --profile production -p quench-node -p quench-bench`), against
Bun with `BUN_JSC_useJIT=0` and Node v26.10.0 `--jitless`.

Host: Linux 6.18.44-fc-v139 x86_64, 4 CPUs, 15 GiB RAM (cloud container), engines run
sequentially. Three rounds per test (the qualification bar is 11), so this is a landscape,
not a qualification: `report.json` is the runner's report, `summary.txt` the median score
and median max RSS per test.

Geomean over the 33 tests both Quench and Bun completed: Quench 7.23, Bun 16.30.
Quench timed out (600 s) on argon2-wasm; Node has no WebAssembly under `--jitless`.
