# DefinePropertyRecord lane screen

This is a focused candidate screen for the Zod invalid-parse path, not a package qualification result.

## Source and binaries

- Baseline source: `v2-zod` commit `07867f55d0bb65572623a8ba355fdebe1eefba01`.
- Candidate source: the same commit plus the local `dispatch_fast.rs` lane handler and parity test in this change.
- Baseline production binary: `3010c9f68840ad8c451a8c487754d099de3ba2ff9d82823258027772f9c997ef`.
- Candidate production binary: `85e49c86c431d11fe1a246e8fab32852c307dc6209da83df17120bfa9650860b`.
- Both builds used `target/pgo-quench-node.profdata` (`cc489574ff15c556a38fa0961ff9b58e307838d3f82fee500548b8d3f05c9f77`) and `-Cllvm-args=-disable-icp`. The candidate build reported expected PGO hash mismatches for changed runtime functions, so treat this as diagnostic.
- Workload SHA-256: `4c0afdc77df325d0578039bcbce25a6513e6bebca44fee10845b1a1ba16e209d`; package lock SHA-256: `92dbfcb2312b42ddf55a8f0b06c40368544ca4b5e861782ead6770268da3c1b0`.

## Measured evidence

The baseline no-JIT stats show `zod/v4/core/parse.cjs::failure` runs `DefinePropertyRecord` twice for each invalid parse, with 10,498 general-dispatch executions in the 5,000-round workload. The added handler keeps this generic operation inside the fast lane while using `lane_in_place` to publish the exact PC, materialize frameless activations, and preserve allocation, throw, and GC behavior.

| Zod no-JIT configuration | Candidate median | Control median | Candidate/control |
| --- | ---: | ---: | ---: |
| 5,000 rounds, 11 samples | 107,527 ops/s | 108,108 ops/s | 0.995× |
| 50,000 rounds, 11 samples | 108,992 ops/s | 105,597 ops/s | 1.032× |

For the 50,000-round samples, the paired candidate/control median was +2.67% (8 of 11 pairs positive). The original 5,000-round acceptance configuration remains within timer and host noise and shows no win. JIT at 5,000 rounds was 106,952 vs 105,820 ops/s (1.011×). All runs passed output parity. RSS fields are diagnostic and this change does not establish a memory gate pass.

Raw matched reports: `zod-no-jit-matched-11.json`, `zod-no-jit-matched-50k-11.json`, and `zod-jit-matched-11.json`.

## Shared JIT prerequisites

Current reverted-source counters in `tests/npm-perf/reports/reverted-prefix-hotpc-2026-10-25/pino-jit-stats-1.json` and `zod-jit-stats-1.json` show:

- Zod `core/memoizer.cjs::isRecursive`: 13 backward sites, all rejected at `LoadEnvLocal`, zero compile attempts/native runs. The package-independent captured-local reproducer is `shared_osr_reports_zod_like_load_env_local_rejection_by_site` in `crates/quench-runtime/src/vm/tests.rs`.
- Pino `lib/tools.js::_asJson`: 5 admitted sites, 13 lane entries, zero compile attempts/native runs; its hot body has 10,476 dynamic `Call` dispatches. `GetIterator`, `IteratorClose`, and `RequireIteratorResult` occur before the first dynamic call. The existing five-site callback reproducer forces GC every 64 calls and throws a unique object at call 5,050; interpreted/compiled output is `40 5050 true`.

Proposed shared fix: add a generic effect-aware call gateway inside OSR regions. Preserve all live values and the exact continuation PC, materialize activations before guest-visible effects, and resume through the ordinary VM call/throw/GC paths. Avoid a larger prefix before the first call; its earlier attempt broke three existing inlining guards. For Zod's captured binding, either add a guarded environment-slot load/store path with correct rooting and write barriers or keep rejecting it. Admission is useful only if exact-site stats show native executions.

Candidate admission before a full matrix: output, GC, throw-identity, and all three inlining guards pass; affected sites show native runs; matched 50,000-round Pino/Zod medians show a repeatable improvement. Original all-package speed and RSS gates remain unchanged.
