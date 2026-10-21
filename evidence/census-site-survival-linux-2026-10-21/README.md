# Census: allocation-site survival of the first collection

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node` with
`quench-runtime/profile-memory`, run with `QUENCH_MEMORY=1`. Census code is on branch
`cand-gc-survival` (1e04e35c9), not in production.

Each object-like allocation (object, array, function, iterator, regexp and the other
slot-carrying kinds) gets one of three fates, attributed to the bytecode site that
allocated it:

- died young: freed in the first collection after its birth
- died old: freed later
- live: still live at exit

A site is called *pretenurable* when it has at least 64 allocations and at least 90% of
them survive their first collection.

JetStream ran 5 iterations. Frameworks used the scorecard driver. See `summary.txt` and
`raw/`.

## Findings

- Survival of the first collection varies widely by workload: 1.5% (lazy-collections),
  2–7% (raytrace, chai, web-ssr), 18–33% (acorn, deltablue, doxbee, earley-boyer), and
  52–83% (frameworks at startup, hash-map, splay, json-parse).
- Pretenurable sites hold less than 3% of survivors in 7 of 16 workloads, and 9–45% in
  most of the rest. The exception is source-map-wtb at 97.8%: four sites survive 100%.
- The high-survival workloads have their survivors at sites with 75–85% survival.
  splay's three insert sites survive 80% (20% die young, 50% die old, 30% live).
  json-parse's parse site survives 80%.

## Consequence for policy

Quench does not move objects: marks are sticky and nothing is copied. Pretenuring would
therefore only save the first minor mark of each survivor and its share of the young
pacing weight. Pretenuring an 80% site turns the other 20% into floating garbage until the
next full collection. That raises RSS, which the memory-first rule does not trade.
Survival-adaptive young sizing has already been covered by the pacing sweeps (TOD-161),
with no gain. Per-site pretenuring is not queued. source-map-wtb is the only workload
where it would apply cleanly.
