# Quench benchmark harness

This directory contains a vendored copy of the `js-engine-benchmark` repository as a git submodule and a small runner for executing the V8-v7 suite on `quench-runtime`.

## Run on quench-runtime

```sh
node quench-bench/run-quench-runtime.mjs
```

Optional flags:

- `--binary PATH` use a specific `quench-node` binary
- `--out FILE` write the benchmark JSON snapshot to `FILE`
- `--only suite1,suite2` run only selected suites (e.g. `richards,deltablue`)
- `--help` show usage

You can also run from npm script:

```sh
npm run bench:quench-runtime
```

The runner materializes `base.js`, each selected fixture, and the V8-v7 suite
runner into a temporary script. A sample is valid only when the process exits
successfully and prints a parsed `Score:` line; empty startup-only executions
are rejected.

For throughput measurements, build the exact binary once and pass it explicitly:

```sh
cargo build --profile bench-throughput -p quench-node
node quench-bench/run-quench-runtime.mjs \
  --quench target/bench-throughput/quench-node \
  --only richards,deltablue \
  --runs 3
```
