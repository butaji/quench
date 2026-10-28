# TSF Wasm JIT callout attribution

This is a one-round diagnostic screen on clean source revision
`adf1b4077464f2b6932d2142a24c36da7cb6336a`. It is not qualification or timing
evidence. The `--jit-diagnostics` instrumentation changes runtime cost; use
the separate uninstrumented 11-round report under `../qualification/` for
performance comparisons.

For TSF, Quench compiled 156 of 244 functions, emitted 177,800 code bytes,
mapped 180,224 bytes, and reported 137,017,327 ns of compilation time. It
recorded 2,896,006 compiled-to-VM callouts: 27,006 imported-function calls and
2,869,000 table/reference calls. Reference calls account for 99.07% of this
total. The aggregate remains the sum of both counters. The report indicates
that imported host calls are not the dominant callout path for this fixture.

The common indirect-call signature path already copies parameter types into
an inline `SmallVec<[WasmType; 8]>`; this report therefore does not justify
removing a per-call signature `Vec` clone. The next targeted performance
experiment is a call-site target cache with guards and invalidation for Wasm
table mutation, while preserving existing table bounds, null-target, signature,
and instance-environment behavior. Do not infer a speedup until an uninstrumented
matched-round comparison confirms it.

Reproduction from the repository root after building the production binaries
and setting the pinned Node and Bun executables:

```sh
target/production/quench-bench --fixtures tsf-wasm --suite jetstream3 \
  --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint target/jetstream3-wasm-jit-callouts-adf1b407.checkpoint.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/callout-attribution/jetstream3-wasm-jit-callout-attribution.json
```

The report records the source revision, clean-tree status, executable hash,
JetStream checkout revision, engine versions, mode evidence, fixture output
validation, and the split counters in the Quench process stderr.
