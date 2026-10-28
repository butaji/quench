# Eval source copy removal and current JIT screen

Source revision: `7bf409bfb2bde94a24936369b4b24ae25eb43a3f` on `v2-jit`, clean. The commit keeps the source `JsString` alive and passes its borrowed host text to eval validation and compilation, avoiding a full owned `String` copy. Indirect eval cache behavior retains a cheap shared `JsString` clone.

## Checks

`CARGO_PROFILE_ITERATION_OPT_LEVEL=0 CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=2 cargo check --locked --profile iteration -p quench-runtime --lib --tests` passed. A direct and indirect eval script covering strict eval, regexp literals and a wide-string result matched Node under both Quench interpreter and `--jit`. The full runtime library test binary was not built in this run.

## Matched fixture screens

Three rounds per execution mode on Linux x86_64, 4 logical CPUs, 400% CPU quota, 16 GiB cgroup memory limit. JetStream corpus revision: `06785cf861ac44855f168cbbe829278c2802e6de`. Bun 1.4.2; Node v24.19.0 (`--jitless` for no-JIT, default optimizing JIT for JIT). Outputs validated for all rounds.

| Fixture | Mode | Quench score / RSS | Bun score / RSS | Node score / RSS |
| --- | --- | ---: | ---: | ---: |
| Web-SSR | no-JIT | 15.23 / 115.1 MiB | 52.47 / 120.3 MiB | 58.94 / 158.7 MiB |
| Web-SSR | JIT | 16.64 / 113.3 MiB | 68.23 / 226.0 MiB | 83.08 / 180.4 MiB |
| TypeScript-lib | no-JIT | 0.19 / 186.8 MiB | 0.47 / 190.3 MiB | 0.49 / 304.1 MiB |
| TypeScript-lib | JIT | 0.08 / 187.0 MiB | 1.46 / 392.2 MiB | 1.58 / 350.7 MiB |

These are three-round fixture screens, not full-suite qualification. Web-SSR varied materially between two adjacent 3-round samples, so no causal speedup is attributed to the copy removal. TypeScript-lib JIT was substantially slower than the interpreter in these samples; the earlier clean 11-round report at the previous code revision is preserved separately.

## TypeScript-lib JIT diagnostics

One diagnostic round (instrumented; do not compare its score) recorded 1,208 functions: 27 compiled, 139 unsupported, 90 hot, 243 reclaimed and 709 lane-only. The 27 compiled functions accounted for 11,633,642 native entries, 110,592 mapped code bytes and 59.1 ms cumulative compile time. Hot inclusive interpreter rows included `parseTokenNode` at 475 ms (27,226 general-dispatch calls) and `doJSDocScan` at 208 ms (9,903 calls); inclusive times overlap nested callees. This points toward general effect-aware call lowering in hot compiled regions, but does not predict the performance outcome.

## Raw reports

- `web-ssr-nojit-3rounds.json`
- `web-ssr-jit-3rounds.json`
- `typescript-lib-nojit-3rounds.json`
- `typescript-lib-jit-3rounds.json`
- `typescript-lib-jit-diagnostics-1round.json`
