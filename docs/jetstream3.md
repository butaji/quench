# JetStream 3 measurement

The corpus is the `quench-bench/jetstream` submodule: `WebKit/JetStream`,
branch `JetStream3.0`, at the commit pinned by the repository. Initialize it
with `git submodule update --init quench-bench/jetstream`.

`quench-bench --suite jetstream3` runs JetStream's own `cli.js` driver, one
test per engine process, in the same matched-round runner, report schema,
checkpoint/resume and analysis as [V8-v7](v8_v7.md):

```sh
cargo build --profile production -p quench-node
cargo run --release -p quench-bench -- --preflight-only --suite jetstream3 \
  --quench target/production/quench-node --node node --bun bun
cargo run --release -p quench-bench -- web-ssr --suite jetstream3 --runs 11 \
  --timeout-ms 600000 --quench target/production/quench-node --node node --bun bun \
  --out target/jetstream3-web-ssr.json
cargo run --release -p quench-bench -- --all --suite jetstream3 --runs 11 \
  --timeout-ms 600000 --quench target/production/quench-node --node node --bun bun \
  --checkpoint target/jetstream3.checkpoint.json --out target/jetstream3.json
cargo run --release -p quench-bench -- --analyze target/jetstream3.json \
  --out target/jetstream3-analysis.json
```

A fixture is a JetStream test name. `--all` selects the lane's test list in
`quench-bench/src/jetstream.rs`: the server-side JavaScript tests first, then the
server-side Wasm tests. The Score is the test's own JetStream line
(`<test> Score <value> pts`): the geometric mean of its First, Worst and
Average scores, computed by JetStream's driver. Maximum RSS comes from the
runner's process accounting, as for V8-v7.

JetStream targets the d8, jsc and SpiderMonkey shells. The runner supplies the
same shell surface to Node-compatible engines through a prelude on `node:vm`:
`print`, `printErr`, `read`/`readFile`, `load`, and `runString`, which creates
a fresh realm for each benchmark exactly as JetStream does in shells. The
driver runs with `--no-prefetch`, so each benchmark realm loads its resources
on demand. Engines therefore need no WebAssembly to decompress prefetched
resources; Node `--jitless` has none. Iteration counts, validation and scoring
are JetStream's and are never altered.

The no-JIT bars are Node `--jitless` and Bun with `BUN_JSC_useJIT=0`, with the
same proofs as V8-v7. QuickJS is not measured: it has no realm API, and
JetStream evaluates every benchmark in a fresh realm.
