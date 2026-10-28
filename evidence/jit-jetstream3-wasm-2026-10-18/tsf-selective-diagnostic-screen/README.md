# TSF selective-tier diagnostic screen

This is a one-round `--jit-diagnostics` screen with selective Wasm tiering
enabled. It is not a matched comparison or qualification result. The report
marks TSF valid with equal outputs, but `qualification_ready` is false. It
records Quench at 8.32 Score and 226.0 MiB, Bun at 209.48 and 133.3 MiB, and
Node at 216.58 and 140.5 MiB. These values are not comparable to the
non-diagnostic 11-round default-JIT report; during the Quench run the report's
host snapshot shows an active `rustc` test build. Use this file only as
historical diagnostic provenance, not as evidence of a tiering effect.

Reproduction after building the production binaries, with no concurrent builds
or benchmark processes:

```sh
target/production/quench-bench --fixtures tsf-wasm --suite jetstream3 \
  --jit --jit-wasm-selective --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/tsf-selective-diagnostic-screen/jetstream3-tsf-selective-diagnostic.json
```

The report records source revision `3dda94daca038690b5e59498692023f945efa272`,
a dirty worktree, the pinned JetStream corpus, one sample per engine, and the
host process snapshots. No causal claim should be drawn from this run.
