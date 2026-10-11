# Splay single-space 40 B record screen

This fixed-work M4 diagnostic evaluates an in-progress uniform-slot layout. It
is not a stock-harness qualification result, and the candidate was rejected by
the all-eight no-regression guard.

## Representation

Source base: `883e5c00fe359e51cdd81d8ac5b0f69701d2b3f3`. The candidate keeps the
existing untagged heap slot index and one `SlotArena`. `Object` is 32 B;
`Option<Cell>` is 40 B. Array elements share an Object storage union, and rare
object extras move to boxed out-of-line storage. There is no second record
space or per-handle space decode.

The complete candidate diff is preserved in
[`single-space-candidate.patch.gz`](single-space-candidate.patch.gz), SHA-256
`38d5dfb210bb5e8d0bb2221c6868ce99dbd005cdecd00cb888184d08c038c9e5`.

## M4 fixed-work Splay screen

Raw report: [`splay-fixed-work-11.json`](splay-fixed-work-11.json).

- Host: Apple M4, arm64 macOS, 10 logical CPUs; rustc 1.99.0, LLVM 23.1.2.
- Work: pinned V8-v7 Splay fixture, 300 `run()` calls per sample, 11 alternating
  baseline/candidate rounds; 11/11 valid and clean, with equal output.
- Runner SHA-256: `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.
- Baseline production binary SHA-256:
  `127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`.
- Candidate production binary SHA-256:
  `d2fa80e84cf9089788103370a999ae803150d802cac95527116d5a5f75d2378d`.

| M4 metric | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Marginal cycles / `run()` | 5,762,545.45 | 5,635,866.88 | −2.20% |
| Marginal instructions / `run()` | 33,092,972.67 | 33,070,386.42 | −0.068% |
| Work max RSS | 122.70 MB | 97.26 MB | −20.7% |
| Setup-only max RSS | 68.78 MB | 55.74 MB | −19.0% |

Using the resumable guard's per-round marginal comparison and seeded 20,000
replicate bootstrap, the median paired changes are −2.382% cycles (95% interval
−3.287% to −1.208%), −0.057% instructions (−0.096% to −0.046%), and −20.759%
work max RSS (paired median; −20.880% to −20.650% observed pair range).

Against the previously measured M4 references (Node `--jitless` marginal
cycles 4,678,660.31; Bun no-JIT max RSS 58,753,024 bytes), this isolated Splay
screen gives provisional distances of 1.20x speed and 1.66x RSS. The references
are from the pre-candidate Step 0 report, so this is not a same-campaign or
stock-harness qualification.

## All-eight guard decision

Raw report: [`all-eight-fixed-work-11.json`](all-eight-fixed-work-11.json).
Decision checkpoint: [`all-eight-guard.decision.json`](all-eight-guard.decision.json).
All eight fixtures had 11/11 valid, uncontended pairs and matching outputs;
"clean" describes sample quality, not acceptance. The layout guard **rejected**
the candidate:

| Fixture | Median cycles | 95% interval | Instructions | Result |
| --- | ---: | ---: | ---: | --- |
| Crypto | +3.772% | [+1.976%, +7.707%] | +1.435% | Regression |
| NavierStokes | +4.748% | [+3.958%, +6.543%] | +1.938% | Regression |

The other six fixtures had no detected cycle regression; RSS fell on all eight.
The layout did not merge. The trunk distances therefore remain **1.24x speed
and 2.09x RSS** on M4, from the post-merge Step 0 baseline.

The candidate's setup-only max RSS was 55.74 MB, versus Bun's previously
measured setup-only 50.73 MB: a 5.01 MB setup gap. This separates the remaining
work-phase high-water from the setup floor, but the two values were not
collected in one campaign.

## Correctness

Before the performance screen, `cargo test -p quench-runtime --features
profile-memory` passed 547 tests and `cargo test -p quench-wasm` passed 31
tests. The Node v26.10.0 array-semantics probe matched byte-for-byte. WAST
discovery and Test262 inputs are absent from this checkout.
