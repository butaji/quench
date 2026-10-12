# Splay mark-bit lookup order screen — Linux, 2026-10-11

## Decision

Rejected. The candidate checked the mark bitmap before the slot arena lookup,
with a bounds-checked mark-word read. It preserved output in all 11 fixed-work
rounds, but setup-adjusted raw wall time regressed 13.98% at the median (3/11
wins; -42.93% to +79.53%). Work RSS increased by 286,720 B and setup-adjusted
RSS by 40,960 B. The Linux runner had no usable hardware counters (0/11 clean
rounds), so this is directional evidence only. No stock Score run was warranted.

This does not move the Splay Score or RSS gates. The candidate is not retained.

## Research basis

V8's documentation describes minor collection as tracing young objects from
roots and remembered old-to-young references. This lane already uses that
architecture. The experiment tested the narrower inference that reading the
mark bitmap before touching a slot could make duplicate edges cheaper. The
screen rejects that inference for this implementation; fewer memory accesses
were not demonstrated by the measurements.

Sources: [V8 garbage collection](https://chromium.googlesource.com/v8/v8/%2Bshow/lkgr/docs/heap/garbage-collection.md),
[Orinoco parallel Scavenger](https://v8.dev/blog/orinoco-parallel-scavenger).

## Build and measurements

- Baseline source commit: `c7390f849255bb4040d87d01c96ca8ef68b0a6c6`.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs / CPU quota 4, 16 GiB
  memory limit; RSS via Linux `wait4`.
- Fixture SHA-256: `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`.
- Runner SHA-256: `924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb`.
- Baseline binary SHA-256: `0e48bf7c26545e16100853840df0f1989882b02ca077d3491fb19eac02a54ba0`.
- Candidate binary SHA-256: `0106765f6333307db08515d942b26f6f9f2fffc1d5010cf032e2bfc6dab117d4`.
- Output matched in 11/11 pairs, with 300 Splay iterations per sample.
- Median work RSS: 120,856,576 B baseline; 121,204,736 B candidate.
- Median setup RSS: 65,097,728 B baseline; 65,368,064 B candidate.
- Raw setup-adjusted median wall delta: +13.98% (candidate slower).

The report stores `wall_ns`, RSS, host load, and contention diagnostics for all
pairs. The patch is kept only to document the rejected screen.

## Reproduction

Starting from `c7390f849`, apply `candidate.patch`, then build and run:

```sh
source /workspace/quench-build-env.sh
cargo build --locked --offline --profile production -p quench-node --bin quench-node
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-v2-cloud-slotgen-early-bitcheck-baseline \
  --quench-peer work/task94-v2-cloud-slotgen-early-bitcheck-candidate-safe \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-v2-cloud-slotgen-early-bitcheck-safe-fixed-11.json
```

Raw compressed report: `fixed-work-11.json.gz`.
