# Dynamic receiver fields in shared JS OSR

This Linux x86_64 experiment follows the `v2-merged` sync at `82fdaf437d5fdc39a25ab112765b6f9bf60fd3f3`. It tests a direct one-step `GetField` whose receiver register changes inside a loop. Native code probes the existing field cache with the current rooted receiver. A miss returns to the lane at that exact instruction, before an accessor, proxy trap, or other guest effect can run.

The synchronous probe also holds one `Rc<ResidualProgram>` and a snapshot of the diagnostics flag in its invocation payload. This removes the per-read `ProgramStore::get` lookup/refcount and diagnostics-enabled lookup. It leaves `cached_field_hit`, cache invalidation, receiver semantics, the native entry ABI, and exact-PC recovery unchanged. The payload and its `Rc` are dropped when the native region returns.

## Correctness

- `cargo test --profile iteration -p quench-runtime --lib`: **976 passed, 0 failed** with `CARGO_PROFILE_ITERATION_OPT_LEVEL=0 CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=2`.
- Focused changing-receiver/effect test: **272 native region runs**, 17,175 cache probes, 17,172 hits, three exact-PC exits; getter/proxy counts and thrown error identity match the interpreter.
- Pinned Node v26.12.0 JIT oracle v1: **8/8 fixtures passed** after the change, with native-entry assertions. `field-changing-receiver` reports 65,538 native entries and 65,538 probes (32,768 hits).
- The earlier 116-script historical inventory remains unavailable. The checked-in 8-fixture manifest is a focused gate, not a substitute.

## Production Richards screen

Command for both reports, changing only the Quench executable:

```sh
target/production/quench-bench --fixtures richards --suite jetstream3 --jit --runs 11 --timeout-ms 600000 --quench QUENCH_BINARY --node /workspace/node-v26.12.0-linux-x64/bin/node --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true --out REPORT.json
```

Both reports are 11/11 valid, output-equal, use the same JetStream input and engine order, and have `qualification_ready=false` because they select only Richards. They were run sequentially on the same four-CPU-quota Linux host with production binaries. Exact Quench executable SHA256 values: pre-hoist `7adcd6825c1e53af3826646cdfa8394c349a61205581bbbc43a09360a67fb8e7`; hoisted `feab337a2eee95356e06a675fbb1573756394460c4c44b8dcda83a136d20d8d1`.

| Build | Quench median score | Quench median RSS | Node v26.12.0 JIT | Bun 1.4.3 JIT |
|---|---:|---:|---:|---:|
| Pre-hoist | 42.96 | 33.54 MiB | 32.89 / 58.97 MiB | 578.46 / 46.50 MiB |
| Hoisted probe | 43.62 | 33.32 MiB | 32.91 / 59.17 MiB | 586.30 / 46.55 MiB |

The hoist changed Quench's median by +1.5%; paired by round position, the median difference was +0.48 points and the candidate scored higher in 7/11 positions. This is a small local signal, not a broad qualification result. On this fixture Quench is about 32.6% ahead of Node but still far behind Bun, so the 20%-over-every-runtime goal is not met. Raw runner reports are included above.

The next measured priority is effectful call coverage and recovery: source attribution found Zod OSR rejection at `LoadEnvLocal` and Pino loops admitted but with zero native runs through repeated call dispatches. The package lane owns those hot-PC prerequisites; shared JIT work must coordinate with it and preserve call rooting, barriers, reentry, exceptions, inlining recovery, and exact-PC fallback.
