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

These commands use the no-JIT comparison mode by default. To opt into JIT
mode, pass `--jit`; the runner launches Quench with `--jit`, runs Node without
`--jitless`, sets `BUN_JSC_useJIT=1`, and verifies an optimizing Node compile
plus Bun's `useJIT=true` option before timing. For example:

```sh
cargo run --release -p quench-bench -- --preflight-only --suite jetstream3 --jit \
  --quench target/production/quench-node --node node --bun bun
cargo run --release -p quench-bench -- web-ssr --suite jetstream3 --jit --runs 11 \
  --timeout-ms 600000 --quench target/production/quench-node --node node --bun bun \
  --out target/jetstream3-web-ssr-jit.json
```

`--jit` is currently restricted to `--suite jetstream3`. The report records a
top-level `jit_mode` (`no-jit` or `jit`) and each engine's version, effective
arguments/environment, JIT mode, and mode proof. Checkpoints are bound to the
mode, so resume a run with the same `--jit` setting.
Quench's proof records that `--jit` selected the shared VM's compiled mode; it
does not currently count whether a benchmark function entered native code.

A fixture is a JetStream test name. `--all` selects the lane's test list in
`quench-bench/src/jetstream.rs`: the server-side JavaScript tests first, then the
server-side Wasm tests. The Score is the test's own JetStream line
(`<test> Score <value> pts`): the geometric mean of its First, Worst and
Average scores, computed by JetStream's driver. Maximum RSS comes from the
runner's process accounting, as for V8-v7.

JetStream targets the d8, jsc and SpiderMonkey shells. The runner supplies the
same shell surface to Node-compatible engines through a prelude on `node:vm`:
`print`, `printErr`, `read`/`readFile`, `load`, and `runString`, which creates
a fresh realm for each benchmark exactly as JetStream does in shells. Dynamic
`import()` in benchmark code goes through the main context's loader
(`vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER`), resolved relative to the
corpus root. The driver runs with `--no-prefetch`, so each benchmark realm
loads its resources on demand. Engines therefore need no WebAssembly to
decompress prefetched resources; Node `--jitless` has none. Instead, each `.z`
resource must already be inflated next to itself (JetStream's
`npm run decompress`, or a zlib inflate of each file). Iteration counts, validation and scoring
are JetStream's and are never altered.

In the default no-JIT mode, the bars are Node `--jitless` and Bun with
`BUN_JSC_useJIT=0`, with the same proofs as V8-v7. In JIT mode, Node runs
without `--jitless` and Bun gets `BUN_JSC_useJIT=1`; the preflight checks that
each is using its optimizing tier. The preflight also probes each engine for `WebAssembly` with its
measurement arguments. A test JetStream's driver tags `Wasm` is not run on an
engine without it: the fixture records the engine under `unsupported_engines`
with the reason, and the analysis measures that fixture only against the
engines that can run it. Node `--jitless` has no `WebAssembly`, so Bun no-JIT
is the only no-JIT bar for the Wasm tests. QuickJS is not measured: it has no realm API, and
JetStream evaluates every benchmark in a fresh realm.
