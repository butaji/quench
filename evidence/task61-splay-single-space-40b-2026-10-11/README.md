# Splay single-space 40 B record experiment

The first candidate screen below is retained as history, but its all-eight
guard rejected that source revision. A later source refinement removes one
array-store mode check; it has correctness evidence but no performance result
yet. Do not use the first screen's speed or RSS distances for the revised source.

## Representation

Source base: `883e5c00fe359e51cdd81d8ac5b0f69701d2b3f3`. The candidate keeps the
existing untagged heap slot index and one `SlotArena`. `Object` is 32 B;
`Option<Cell>` is 40 B, enforced by `cells_stay_compact`. Array elements share
the Object storage union, and rare object extras move to boxed out-of-line
storage. There is no second record space or per-handle space decode.

## Initial M4 fixed-work pair — superseded and rejected

Raw report: [`splay-fixed-work-11.json`](splay-fixed-work-11.json).

- Host: Apple M4, arm64 macOS, 10 logical CPUs; rustc 1.99.0, LLVM 23.1.2.
- Work: pinned V8-v7 Splay fixture, 300 `run()` calls per sample, 11 alternating
  baseline/candidate rounds; 11/11 valid and clean, with equal output.
- Runner SHA-256: `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.
- Baseline production binary SHA-256:
  `127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`.
- Candidate production binary SHA-256:
  `d2fa80e84cf9089788103370a999ae803150d802cac95527116d5a5f75d2378d`.
- Candidate source diff SHA-256: `38d5dfb210bb5e8d0bb2221c6868ce99dbd005cdecd00cb888184d08c038c9e5`.

| M4 metric | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Marginal cycles / `run()` | 5,762,545.45 | 5,635,866.88 | −2.20% |
| Marginal instructions / `run()` | 33,092,972.67 | 33,070,386.42 | −0.068% |
| Work max RSS | 122.70 MB | 97.26 MB | −20.7% |
| Setup-only max RSS | 68.78 MB | 55.74 MB | −19.0% |

Using the resumable guard's per-round marginal comparison and seeded 20,000
replicate bootstrap, the median paired changes are −2.382% cycles (95% interval
−3.287% to −1.208%), −0.057% instructions (−0.096% to −0.046%), and −20.759%
work max RSS (paired median; −20.880% to −20.650% observed pair range). All 11
pairs pass the host-contention checks.

Relative to the previously measured M4 references (Node `--jitless` marginal
cycles 4,678,660.31; Bun no-JIT max RSS 58,753,024 bytes), the initial
candidate measured at 1.20x the speed bar and 1.66x the RSS bar. The reference measurements
are from the pre-candidate Step 0 report, so these are provisional distances,
not a same-campaign re-take.

The initial all-eight guard is
[`all-eight-guard.decision.json`](all-eight-guard.decision.json). It rejected
the candidate because Crypto and NavierStokes had clean instruction regressions
of +1.435% and +1.938%, respectively, alongside cycle regressions of +3.772%
and +4.748%. Splay itself improved, but the all-eight guard applies.

## Revised candidate status

The revised source changes the dense-array store to read indexed-descriptor
state from the already-matched `Cell::Array` record, removing a generic storage
mode check. Static disassembly confirms that the `ARRAY_ELEMENTS` bit test is
absent from that store preamble. The expanded Node oracle probe then exposed an
inherited non-writable array-index mismatch in the generic `Reflect.set`
property walk: `own_property` did not project array elements. That shared view
now includes present dense and sparse array elements, checking for the Array
receiver before parsing the key, with a regression test. This fixes the
observed semantic mismatch; it does not establish a fixture-level performance
benefit.

- Revised source diff SHA-256: `6d16656e8a1ebe7ff22f977116536a3c1f873eb10bc1a5ea1118e6d2fbadd767`.
- Revised production `quench-node` SHA-256: `7331fe083eba26249a4955764419a6695ec708aaf391df37645cb254544effc5`, pinned at `target/pinned/7331fe083eba26249a4955764419a6695ec708aaf391df37645cb254544effc5/quench-node`.
- Build: `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/candidate-a cargo build --profile production -p quench-node --bin quench-node`, rustc 1.99.0, Apple M4 arm64.
- `cargo test -p quench-runtime --features profile-memory`: 548 passed; 0 failed.
- `cargo test -p quench-wasm`: 31 passed; 0 failed.
- `crates/quench-runtime/tests/subset.rs`: 6 passed; 0 failed.
- The expanded Node v26.10.0 `--jitless` array-semantics probe now matches byte-for-byte after the `own_property` fix.
- The Splay fixed-work attempt below has 0/11 clean rounds; the all-eight guard has not been rerun on this revised source.

## Revised-source Splay attempt — contention rejected

Raw report: [`splay-fixed-work-11-revised.json`](splay-fixed-work-11-revised.json).
It used the pinned CJS fixed-work runner, 300 `run()` calls per sample, and 11
alternating pairs of baseline binary `127b3fdc…` and revised candidate binary
`7331fe08…`. All 11 rounds were output-valid, but 0/11 passed the contention
detector. Sampled load ranged from 1.676 to 3.094 runnable tasks per logical
CPU, so cycles, instructions, and elapsed-time deltas are unavailable.

The valid-process max-RSS medians were 113.66 MiB baseline vs 90.50 MiB
candidate for the work sample (−20.37%), and 63.83 MiB vs 51.52 MiB for
setup-only (−19.29%). Treat these as directional observations from a
contended campaign, not as the RSS gate: no paired clean-sample decision was
made. The candidate's speed and RSS distances therefore remain unqualified.

The clean-trunk M4 distances remain **1.24× speed / 2.09× RSS**. The revised
candidate's distances are unmeasured and must be established with the
production-CJS fixed-work gate after the host-contention detector is clean.

## Initial candidate correctness and remaining requirements

For the initial source, the runtime and Wasm suites passed before timing, and
the smaller Node array-semantics probe matched byte-for-byte. The expanded
probe found the inherited non-writable index case, now covered by the revised
source's matching output and unit test. WAST discovery and Test262 inputs are
absent from this checkout. A retained revision still needs the Splay gate, the
all-eight fixed-work layout guard, and eventually a stock-harness qualification
campaign.
