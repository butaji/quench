# TOD-158: cheaper mark bookkeeping per collection and per object

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota. Release builds of
`quench-node`.

- Base `b590`: v2-gc `c590a05f`.
- Candidate `c158`: `9c9c5b4b`, this change only on top of `c590a05f`.
- Hashes are in `binaries.sha256`.

## Change

- The shape-constructor memo is extended incrementally (only shapes added since the last
  collection) instead of being rebuilt every collection. It is cleared when shapes compact.
- A per-collection `OwnedRootFilter` (16 bits per owner) gates the `owned_roots` hash probe.
- The constructor-shape bit is loaded before `fetch_or`.

## Decision rule (set before measuring)

Keep the change if all of these hold:

1. Collector instructions fall on the mechanism workload (lazy-collections).
2. No paired workload loses beyond noise (consistent 0/N together with a wall-time loss).
3. Peak RSS is unchanged within ±1 MB or 2%.

## Result: kept

| Measure | Base | Candidate |
| -- | -- | -- |
| lazy-collections instructions, callgrind 4 iterations (`callgrind-lazy-collections.txt`) | 25.17 G | 22.13 G (−12%) |
| `collect_slow` inclusive | 6.06 G | 4.79 G (−21%) |
| JetStream suspects, 5 pairs (`jetstream-suspects-exact-base-5.txt`) | — | geomean ×1.025, RSS ×1.002; bigint ×1.096 (5/5), zlib ×1.055, the rest within noise |
| typescript-lib, 2 pairs | 587.6 s, 254.2 MB | 586.9 s, 253.8 MB (Score ×0.964 is two-decimal rounding of 0.28 vs 0.27) |

`scorecard-vs-c151e-*.txt` compare against an earlier v2-gc build (`c151e`, before the
`c590a05f` upstream merge). Their apparent losses on these tests reverse against the exact
base, so they belong to the merge, not to this change:

- source-map-wtb ×0.859, espree-wtb ×0.938, zlib-wasm ×0.975, each 0/5.
- typescript-lib RSS 240 → 254 MB.

The same files also show the V8-v7, CoreMark and framework pairs. Against `c151e` those are
within noise: crypto ×1.108 and earley-boyer ×1.107 (3/3); regexp, raytrace and splay
×0.95–0.96 (1/3).

## Tool

`scorecard.py BASE CAND ROUNDS [v8,coremark,fw,js3]` runs interleaved, order-alternating
quench-node pairs over:

- V8-v7;
- JetStream 3 at `--iteration-count=15` (`JS3=` selects tests);
- Wasm CoreMark (`coremark-minimal-mvp.wasm` from wasm-coremark-rs);
- the framework scenarios in `tests/frameworks`: express, koa and fastify are pinned, hono
  and h3 are installed with `npm install --no-save`.

Each run is its own process under the `posix_spawn` + `wait4` launcher. For each workload it
reports the paired speed ratio, wins, and median max RSS separately.
