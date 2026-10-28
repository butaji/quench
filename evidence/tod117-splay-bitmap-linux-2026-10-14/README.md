# Derive live shapes from the mark bitmap

The candidate replaces the per-record `live_shapes` vectors emitted by serial
and parallel sweep with a shape bitmap populated by the existing mark callback.
This avoids a duplicate list of reachable shapes while retaining shape
compaction, including shapes that have no GC-bearing properties. The source
candidate is based on `6c2771a7f`; its optimized executable SHA-256 is
`3696f72c38b44bdb2de7d2a0e021be7c88dd900a5ba302ca888aca05b653a1a6`. The
baseline executable SHA-256 is
`b638f8edb57f54854aedcb4af1b94c6ef087634bc7e2b76ae39c21b3befb6789`.

Correctness passed: the runtime suite reported 585 unit tests and 9 subset
tests passing, including the full-mark verifier, parallel marking, and the
rootless-shape compaction regression. Both fixed-work Splay reports and the
all-eight report are output-equal.

## Splay screen

The host was x86_64 Linux, kernel `6.18.44`, with a 4 CPU cgroup quota and a
16 GiB memory limit. The V8-v7 corpus was pinned at
`64e1860c736c1b899708f4cd646721bc71e53d8b`.

| 11-pair screen | Median paired wall delta | Bootstrap 95% interval | Median max RSS delta | Clean pairs |
| --- | ---: | ---: | ---: | ---: |
| Unpinned | +0.75% | −7.07% to +1.98% | −3.90% | 0/11 |
| Pinned to CPU 4 | −4.20% | −8.93% to −1.02% | −3.89% | 0/11 |

The intervals are bootstrap intervals over the 11 valid raw paired wall ratios.
The runner marked every pair contended, so the pinned result is a promising
screen rather than a qualified clean-pair result.

The all-eight initial report contains 7 valid pairs per fixture and 0 clean
pairs for every fixture. Its median max RSS deltas were Crypto 0.000%,
DeltaBlue −0.363%, EarleyBoyer −0.813%, NavierStokes −0.354%, RayTrace
+0.358%, RegExp 0.000%, Richards 0.000%, and Splay −3.905%. The resumable
layout guard decision is **inconclusive**: this Linux runner reports no cycles
or instructions, and this environment has no `perf` executable. The layout
guard requires seven clean cycle pairs and does not allow an instructions-only
fallback. No lane push or merge is made on this evidence.

The paired reports are suitable for reproducing the focused screen, but the
all-eight result must be retaken on a host that can provide the required
performance counters and clean pairs before this candidate can be published.

## Reproduction

```sh
cargo run --release -p quench-bench --bin quench-bench -- \
  quench-bench/js-engine-benchmark/v8-v7/splay.js --fixed-work --runs 11 \
  --quench target/pinned/baseline/quench-node \
  --quench-peer target/pinned/candidate/quench-node \
  --out target/tod117-splay-bitmap-pair.json

taskset -c 4 target/release/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js --fixed-work --runs 11 \
  --quench target/pinned/baseline/quench-node \
  --quench-peer target/pinned/candidate/quench-node \
  --out target/tod117-splay-bitmap-pair-cpu4.json

target/release/quench-bench --all --fixed-work --runs 7 \
  --quench target/pinned/baseline/quench-node \
  --quench-peer target/pinned/candidate/quench-node \
  --out target/tod117-all-eight-initial.json

cargo run --release -p quench-bench --bin task61-fixed-work-guard -- \
  start --initial-report target/tod117-all-eight-initial.json \
  --checkpoint target/tod117-all-eight-guard.json --change-kind layout
```

Compressed raw reports and guard state are retained here. Their SHA-256 values
are:

- `splay-pairs-unpinned.json.gz`: `fdc03b74a83ca3dc269e567b000ed56689a76aab7f0dd040f3adfc0a0e02ed72`
- `splay-pairs-cpu4.json.gz`: `4424cf68cbfc292bf470243647efdb3a50949cafce67467c0cd2cd84694ca115`
- `all-eight-initial.json.gz`: `35ab969c7c6fc067c3e312d9ff6db0ad291a2d8ba7db01447250cd1e230f064a`
- `tod117-all-eight-guard.json`: `36d6a9dbf90675b3a03de685c5c7b379247896e21b0d8b3d3a8e09a2a5b32630`
- `tod117-all-eight-guard.decision.json`: `dbc2897dd72d230e7373af75056fbcb01363298a5ed301369a7df632df5d798b`
