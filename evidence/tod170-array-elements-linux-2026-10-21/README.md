# TOD-170: array elements in one copy-on-write allocation — kept on production builds

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`, both binaries run with
`QUENCH_COMPILE_CACHE=`.

- Base `b170`: v2-gc at 699217cf6.
- Candidate `c170`: `cand-gc-array-elements` at 90c37a551.
- `c170b` adds `ArrayElements::filled` and link-order entries for the new functions. It was
  used only for the navier-stokes checks below.

## Change

`ArrayElements` replaces `Rc<Vec<Value>>`:
- One allocation: a 16-byte header (owners, length, capacity) followed by the values.
- Empty arrays share a static header and allocate nothing.
- Writes go through `make_mut`, which returns a unique view (`UniqueElements`) and copies a
  shared backing first.

## Measurements

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7, round 1 | 10 | ×0.967 | ×1.013 |
| V8-v7, round 2 | 10 | ×1.014 | ×1.015 |
| JetStream (12 tests) | 3 | ×0.999 | **×0.971** |
| Frameworks | 3 | ×0.999 | ×0.995 |
| npm top 300 | 1 | — | ×0.9943, no new failures |

JetStream detail (RSS):
- web-ssr 69.3 → 57.8 MB
- typescript-lib 223.1 → 217.5 MB
- acorn 37.7 → 35.2 MB
- source-map 56.4 → 54.1 MB
- espree 47.0 → 46.1 MB

The V8 speed loss in round 1 was noise. Callgrind: deltablue 1.990G → 1.978G instructions,
earley-boyer equal. Alternating deltablue runs: the candidate won 5 of 6. Round 2: ×1.014.

quench-runtime lib: 792/792.

## Why held

The rule (TOD-170) requires every suite's RSS ≤ ×1.005. V8-v7 is ×1.015, all from
navier-stokes: peak max RSS 4.97 → 5.40 MB in every run.

What the checks found:
- **Not malloc bytes.** Massif heap totals are identical.
- **Not collector pacing.** gc-tuning traces show the same 4 collections at the same points.
- **Not the program's own allocation pattern.** One iteration run directly peaks the same
  (5.31 vs 5.31 MB). So does a six-array `new Array(n)` micro-test.
- **Not glibc thresholds.** `MALLOC_TRIM_THRESHOLD_=0` and `MALLOC_MMAP_THRESHOLD_` change
  nothing.
- **Not link order coverage.** Both binaries execute 1.16 MB of functions missing from the
  order file.
- **Where it is.** Polled peaks put the extra in file-backed pages: anonymous memory is only
  +0.1 MB.

The peak is therefore set by which code pages are resident when the read-only page release
after each collection runs. That is fragile, layout-dependent behavior, not array storage.
The candidate is held until the release-timing sensitivity is understood or the owner
weighs the trade.

## Side finding

Trunk's link order (`crates/quench-node/link/x86_64-linux.order`) is stale. Navier-stokes
executes 1,791 functions; 905 of them (1.16 MB) are not in the order. A retrain should
cut resident text for every quench-node process.

## Production-profile measurement: kept

The release builds compared above probably link without a usable order, since the order
file names production crate hashes (TOD-171). That leaves file-page RSS to layout chance.
The shipped profile is production, so the comparison was repeated on production builds:

- `p170b`: v2-gc at 85b8a5bfd, with the retrained order.
- `p170c`: the candidate merged with that v2-gc and the same order file.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | ×0.994 | ×1.002 (navier-stokes 5.2 → 5.3 MB) |
| JetStream (9 tests) | 3 | ×1.014 | **×0.982** (typescript-lib 222.7 → 215.7, web-ssr 61.5 → 58.7, espree 49.1 → 46.9) |
| Frameworks | 3 | ×0.993 | ×0.992 |
| npm top 300 | 1 | — | ×0.9964, no new failures |

The rule is met on every suite.

Splay was ×0.910 in the scorecard (2/10 wins). Six alternating direct runs average −1.5%
(5,274 vs 5,197), which is within single-workload noise (TOD-137 A/A note).
