# Selective Wasm tier screen on zlib

This is a three-round diagnostic subset screen of the existing opt-in
per-function Wasm tier on `zlib-wasm`. It tests whether keeping cold functions
in the shared VM recovers the interpreter's score or RSS after enabling JIT.
The selective screen and its diagnostic run used clean source revision
`7ed0663efad8ffb26c6007f546d51ae19b7d7a38`, the pinned JetStream 3 corpus,
Quench production binaries, Bun 1.4.3, and Node v26.12.0. All selected runs
validated and produced equal output. Three rounds are only a screen; none of
these reports can qualify a JetStream subset.

| Quench mode | Quench Score / RSS MiB | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: |
| JIT, selective tier | 13.76 / 112.76 | 91.37 / 114.53 | 89.69 / 110.66 |
| JIT, default tier | 13.20 / 112.58 | 90.00 / 114.04 | 89.26 / 110.99 |
| JIT, root-only 11 | 12.53 / 112.04 | 86.12 / 114.46 | 88.69 / 111.46 |
| JIT, root-first 11 | 12.71 / 112.77 | 90.55 / 114.25 | 91.91 / 111.42 |
| Interpreter | 12.15 / 93.96 | 7.07 / 43.33 (Bun no-JIT) | unsupported (Node no-JIT has no Wasm) |

Selective mode raised the median score by 4.2% over default JIT in separate
three-round reports, while RSS was effectively unchanged. It still scored
about 15% of either JIT peer. The interpreter control beat Bun's no-JIT score
in this run but used more than twice its RSS; the older 11-round no-JIT
qualification (`evidence/jetstream3-wasm-qualification-linux-2026-10-16/`)
recorded 18.08 / 34.5 MiB for Quench and 6.24 / 46.5 MiB for Bun. Those
different revisions and runtime versions should not be combined as one paired
measurement.

The root-only and root-first reports also validate with equal output. Root-only
uses Cranelift “no optimization” by design; its lower score is not evidence
about optimized root compilation. Root-first uses speed optimization but did
not improve the score or RSS over the default run. Neither admission policy
is a useful zlib setting without a stronger code-generation change.

An opt-in one-round diagnostic attributes 7,704,769 native entries to
`longest_match` (runtime function 11, module function 21), compiled alone in
a 1,719-byte root. Selective admission still made 23 compilation attempts
across the zlib run: 49 functions and 113,064 emitted bytes total, with
91.3 ms summed Cranelift time. Peak RSS remained 112.9 MiB. This points to
compiler working-set or process residency rather than retained code size as
the memory cost. The data do not support making selective admission the
default for zlib.

The selective and diagnostic raw reports are clean-source. The default-JIT
and interpreter control reports were recorded at the same commit and binary
hash after the first raw report had been created in this untracked evidence
directory, so the runner's source-dirty flag reflects that evidence artifact;
the tracked source tree had no changes. The timings are separate reports, not
a randomized paired comparison.

Reproduce the selective screen after building the production binaries:

```sh
target/production/quench-bench --fixtures zlib-wasm --suite jetstream3 \
  --jit --jit-wasm-selective --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/selective-zlib-screen/selective-3-round.json
```

Raw artifacts are `selective-3-round.json`, `default-3-round.json`,
`root11-only-3-round.json`, `root11-first-3-round.json`, `nojit-3-round.json`,
and `selective-diagnostic-1-round.json`.
