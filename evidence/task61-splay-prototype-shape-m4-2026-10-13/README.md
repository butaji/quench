# Splay prototype-in-Shape trial (M4)

This Splay-only layout trial moved ordinary-object prototypes from `Object`
payloads into their root `Shape`. The goal was to reduce per-object storage
and make ShapeTable the single prototype authority. It is **rejected**: Splay
work RSS fell, but the change added executed instructions on Splay and caused
clear all-eight regressions.

## Measurement identity

- Platform: Apple M4 arm64, macOS 15; production profile, interpreter only.
- Source revision: `4062a6be95e53bc600747a72f19c24efa7f4fb61` with the runtime
  patch in `prototype-in-shape-runtime.patch.gz` (SHA-256
  `6d2e24f7851528373400deca5e90afe3f1c6c7faa147b504c95036249ec0d1f0`).
- Baseline binary SHA-256: `098cc7aa69195bab87115b496a4df9660f78c5d7364e60a52c8761f2e857f5e6`.
- Candidate binary SHA-256: `64dd819ab550b15b1056b76c8f1118cabb8ce569cd3b09a3caa7d7a05746f143`.
- Runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Effective production settings included `-C opt-level=3`, panic abort,
  thin LTO, one codegen unit, 64-byte function alignment, and the configured
  macOS deployment target.

The synced Node modules do not currently build warning-denied on macOS. Both
matched binaries therefore used the same temporary macOS signal table and
unused-parameter adjustment (`m4-node-build-compat.patch.gz`, SHA-256
`446f5b924495c5c95cda1644ac1e63fbd230152d6d3a850a7aa0f7dc6cd606af`) plus
`--cap-lints allow`. These are build-only controls; the Splay workload does not
use those Node APIs. They are not part of the runtime candidate.

To reproduce, decompress and apply the Node compatibility patch, then build
the baseline at the source revision above. Decompress and apply
`prototype-in-shape-runtime.patch.gz` for the candidate and build with the same
command and flags:

```sh
RUSTFLAGS='-C link-arg=-mmacosx-version-min=15.0 -D warnings -C llvm-args=-align-all-functions=6 --cap-lints allow' \
  cargo build --locked --profile production -p quench-node --bin quench-node
```

The pinned binaries are under `target/pinned/<sha256>/quench-node`. Fixed-work
reports were produced with the pinned runner and the `--fixed-work --runs 11`
mode; all-eight reports add `--all`.

## Splay fixed-work result

The 11-pair Splay report has 11/11 valid, clean, output-equal pairs at 300
`run()` calls per work sample. Medians and paired bootstrap results:

| Metric | Baseline | Candidate | Paired change (95% interval) |
| -- | --: | --: | --: |
| Marginal wall per run | 888,902 ns | 894,176 ns | +0.593% [−0.782%, +1.168%] |
| Marginal cycles per run | 3,554,192 | 3,578,463 | +0.549% [−0.170%, +1.480%] |
| Marginal instructions per run | 20,230,373 | 20,316,895 | +0.428% [+0.406%, +0.453%] |
| Work max RSS | 117,293,056 B | 105,070,592 B | −12,222,464 B (−10.42%) |
| Setup-only max RSS | 79,659,008 B | 72,990,720 B | −6,668,288 B (−8.37%) |

The active benchmark gained memory but did not gain speed; its instruction
increase is outside the paired interval. No stock Score campaign was run for
this candidate.

## All-eight guard

Same-binary A/A and baseline/candidate A/B reports are preserved beside this
file. The layout guard classified the candidate as rejected. The A/B has
11/11 valid pairs per fixture; clean counts were Crypto 11, DeltaBlue 11,
EarleyBoyer 11, NavierStokes 9, RayTrace 8, RegExp 11, Richards 10, and Splay
6. Regressions exceeded the configured guard on Crypto (+1.00% cycles),
DeltaBlue (+0.69% cycles, +0.60% instructions), EarleyBoyer (+1.36% cycles,
+0.77% instructions), RayTrace (+2.97% cycles, +0.66% instructions), and
Richards (+0.66% instructions). Splay instructions increased +0.42%; its
six-clean-pair cycle interval remained inconclusive. RSS fell on all eight.

The resumable guard stopped with `overall: rejected`; it did not run top-ups
for pending fixtures because other fixtures already exceeded the regression
limits. Its checkpoint and decision are `guard-checkpoint.json` and
`guard-checkpoint.decision.json`.

## Correctness checks

- `cargo check -p quench-runtime` passed on the synced candidate source.
- The optimized runtime library suite passed 580/580 before the latest
  `origin/v2-merged` sync. After sync, the focused debug `prototype` filter
  had 10 passing tests; 23 tests stopped at the direct-record full-mark
  verifier assertion in `heap.rs:1633`. The production test build omits that
  debug-only verifier. No stock benchmark qualification is claimed.

Raw reports: `splay-ab-11.json`, `all-eight-aa-11.json`,
`all-eight-ab-11.json`, and the two guard checkpoint files. The candidate
source patch is retained here for reproducibility; it was not committed as
runtime code.
