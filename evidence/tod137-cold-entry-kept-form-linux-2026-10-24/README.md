# A cold-compiled entry runs from its kept form

## Finding

Each board's first run of a fixture is its worst. The harness writes the fixture to a fresh
path, so that run compiles it cold. The cold path compiled the entry with its records kept
(`keep_records`), stored the encoded program in the compile cache, and then ran the fresh
compilation: every function's record stayed compiled in memory for the whole run.

A warm run instead decodes each record on first call, from the cache entry that the host maps
read-only and pages out at collection boundaries.

## Change

`prepare_embedding_specialized_script_retaining` now does two things:
- Once it has stored the cold compilation, it drops the compilation and loads the entry
  through `kept_script`, the same decode a warm run uses.
- If the store or the reload fails (no cache directory, a full disk), it falls back to the
  compiled program.

Semantics are those of every warm run. The warm-path helper is shared, not copied.

## Measurement

`coldwarm.py`: 5 cold/warm pairs per row. Each pair copies the fixture to a fresh path, then
runs it twice: cold, then warm. MemoryFirst, binaries evicted. Static production builds,
unbolted; the base is v2-gc `d87c06f3e7`. Medians in KiB:

| Row | Base cold | Base warm | Change cold | Change warm |
| -- | --: | --: | --: | --: |
| NavierStokes | 4708 | 4212 | **4172** | 4188 |
| NavierStokes `--jit` | 4528 | 4496 | 4480 | 4480 |
| Crypto | 3480 | 3276 | **3292** | 3208 |
| Crypto `--jit` | 5752 | 5624 | 5608 | 5548 |
| RayTrace | 3332 | 2968 | **3016** | 2960 |
| RayTrace `--jit` | 3252 | 3240 | 3300 | 3216 |
| DeltaBlue | 3100 | 3016 | 3080 | 3028 |
| DeltaBlue `--jit` | 3480 | 3284 | **3264** | 3264 |
| Richards | 2968 | 2696 | 3092 / 2964 in repeats | 2736 |
| Richards `--jit` | 3104 | 3016 | 2992 | 2992 |
| RegExp | 6228 | 6072 | **6028** | 6016 |
| RegExp `--jit` | 6452 | 6188 | **6104** | 6180 |

- **Cold runs:** most cold penalties fall to within noise of the warm run.
- **Richards:** its cold peak is bimodal in both builds (2964–3092 over four repeats). It comes
  from elsewhere, probably the entry compile's own working set.
- **Warm runs:** unchanged.
- **Cost:** a whole cold typescript-eslint run (empty cache) takes 6992 ms of CPU on the base
  and 6955 with the change (median of 5).
- **Tests:** runtime 915/915.
- **npm-top300:** typescript-eslint, lodash, semver, chalk, debug, ms, uuid, yargs, commander,
  express and zod pass both cold and warm.
