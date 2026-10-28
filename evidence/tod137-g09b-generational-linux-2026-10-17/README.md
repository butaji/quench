# G-09b: sticky-mark generations with survival-adaptive pacing

Linear: TOD-137 (stage 2), TOD-124. Host: Linux x86_64 cloud VM, 4 vCPU, kernel
6.18.44, rustc 1.99.1, shared guest (wall-clock scores are noisy; ratios below are
paired). Baseline: `v2-gc` at `ae87e42c` (`v2-merged` merged on 2026-10-17, pacing
of `e14aadb13`: first collection at 2^18 cells, 4x live, every collection full).
Candidate: `e4cebb83`. Binaries: `binaries.sha256`. Both are `cargo build --release
-p quench-node` builds with the committed link-order file.

## Change

- Survivors keep their mark bits; a full collection clears them, a minor traces
  roots plus the old owners the write barrier logged.
- One owner-level barrier: `RecordSpace::get_live_mut` (an old record is unmarked
  and logged once), `get_live_mut_storing` (skipped for non-reference stores),
  `Heap::get_mut` for legacy cells, `Heap::write_barrier` for side tables
  (sparse elements).
- One pacing authority (`heap/pacing.rs`): young budget half the live count (at
  least 2^15), full when the old generation doubles or after eight live sets of
  allocation; when more than half of young allocations survive, full collections
  with four live sets between them, probing survival every fourth collection.
- Cheaper minors: program roots traced only when changed, only young weak
  containers pruned, startup-intrinsic roots derived once.

## Correctness

- Runtime lib suite: 686 passed at `e4cebb83` (697 after merging `v2-merged`),
  with the minor verifier comparing every minor with a full trace with old
  objects pinned. The verifier found one barrier gap on its first run (sparse
  element stores into record arrays), fixed through `write_barrier`.
- All eight V8-v7 fixtures complete with debug assertions on (verifiers on
  every collection): `verifier-run.txt` (scores are of the checked build).

## Stock pairs (9 interleaved rounds, alternating order) — `stock-pairs-9.txt`

| Bench | Score ratio median [min..max] | Cand ahead | Max RSS base -> cand |
|---|---|---:|---|
| Crypto | 1.023 [0.786..1.188] | 5/9 | 22.0 -> 7.5 MB |
| DeltaBlue | 0.950 [0.716..1.163] | 3/9 | 20.3 -> 6.9 MB |
| EarleyBoyer | 1.048 [0.822..1.193] | 6/9 | 47.6 -> 15.9 MB |
| NavierStokes | 0.972 [0.799..1.064] | 3/9 | 5.9 -> 6.0 MB |
| RayTrace | 1.019 [0.877..1.217] | 5/9 | 18.2 -> 6.3 MB |
| RegExp | 1.000 [0.824..1.224] | 4/9 | 26.8 -> 14.3 MB |
| Richards | 1.055 [0.706..1.375] | 5/9 | 5.8 -> 5.7 MB |
| Splay | 1.016 [0.858..1.474] | 6/9 | 93.7 -> 94.2 MB |

No Score change is distinguishable from this host's noise; every interval
includes 1. Max RSS falls 47% to 67% on the five benches where garbage headroom
set the peak. EarleyBoyer (15.9 MB) is below QuickJS-ng 0.10.1 (17.5 MB, one
round, same host). Splay keeps its Score and RSS through the full-only regime
(its young allocations mostly survive).

## Instruction guard (`callgrind-guard.sh`, fixed work) — `callgrind-guard-raw.txt`

Per-run deltas at the barrier revision before the last two minor-cost changes:
Crypto +1.23%, DeltaBlue +1.08%, EarleyBoyer +5.42%, NavierStokes +1.75%,
RayTrace +1.88%, RegExp -0.31%, Richards +2.17%, Splay attribution invalid (its
collections moved between setup and work; total +5.6%). Richards and
NavierStokes run no collection: their delta is the write barrier on reference
stores (about 8 instructions per store). Decision 24/25b: recorded as a finding,
not a hold.

## Collection phases (`profile-gc-time`, one run)

| Bench | Collections | Mark | Sweep | Root gathering |
|---|---:|---:|---:|---:|
| EarleyBoyer | 452 | 249 ms | 339 ms | 46 ms |
| RegExp | 442 | 55 ms | 519 ms | 6 ms |
| RayTrace | 151 | 13 ms | 44 ms | 3 ms |

Baseline (all full): EarleyBoyer 70 collections, mark 68 ms, sweep 598 ms;
RegExp 59, 57 ms, 603 ms. Sweep (freeing dead cells' malloc'd payloads) is the
next cost to remove (strategy stage 4).

## Reproduce

`pair.py BASE CAND 9 bench...` with `spawn.c` (posix_spawn + wait4) over the
materialized stock fixtures (`base.js` + fixture + runner, as `quench-bench`).
