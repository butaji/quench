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
- The first revised-source Splay attempt below was contaminated; a later clean Splay-only screen is recorded next. The revised-source all-eight guard is pending.

## Revised-source Splay screen — clean, all-eight pending

Raw report: [`splay-fixed-work-11-m4-rerun.json`](splay-fixed-work-11-m4-rerun.json).
The production-CJS runner used 300 `run()` calls per sample and 11 alternating
baseline/candidate pairs. All 11 pairs were valid and clean, and stdout matched.
Runner SHA-256: `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.
Baseline binary SHA-256:
`127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`.
Candidate binary SHA-256:
`7331fe083eba26249a4955764419a6695ec708aaf391df37645cb254544effc5`.

| M4 metric | Baseline | Candidate | Candidate change |
| --- | ---: | ---: | ---: |
| Marginal cycles / `run()` | 5,721,421 | 5,629,649 | −1.60% |
| Marginal instructions / `run()` | 33,087,840 | 33,116,028 | +0.085% |
| Work max RSS | 122,765,312 B | 97,206,272 B | −20.82% |
| Setup-only max RSS | 68,665,344 B | 55,672,832 B | −18.92% |

The paired median changes are −1.117% cycles (bootstrap 95% interval
−1.509% to +0.104%), +0.083% instructions (+0.075% to +0.116%), and
−20.793% work max RSS (−20.877% to −20.767%). Thus the candidate has a clear
RSS reduction and no detected cycle regression on Splay, but the point cycle
delta is not resolved from zero by the paired interval.

Against the prior M4 references (Node `--jitless` marginal cycles 4,678,660;
Bun no-JIT max RSS 58,753,024 B), the fixed-work proxy distances are **1.203x
speed / 1.654x RSS**. These references were not retaken in this campaign, and
this is not the required stock-harness qualification. Splay remains unwon;
the all-eight guard must pass before retaining this layout.

## Earlier revised-source Splay attempt — contention rejected

Raw report: [`splay-fixed-work-11-revised.json`](splay-fixed-work-11-revised.json).
It used the pinned CJS fixed-work runner, 300 `run()` calls per sample, and 11
alternating pairs of baseline binary `127b3fdc…` and revised candidate binary
`7331fe08…`. All 11 rounds were output-valid, but 0/11 passed the contention
detector. Sampled load ranged from 1.676 to 3.094 runnable tasks per logical
CPU, so cycles, instructions, and elapsed-time deltas are unavailable.

Its RSS values were directional only; the clean screen above supersedes that
attempt for the revised source.

## Initial candidate correctness and remaining requirements

For the initial source, the runtime and Wasm suites passed before timing, and
the smaller Node array-semantics probe matched byte-for-byte. The expanded
probe found the inherited non-writable index case, now covered by the revised
source's matching output and unit test. WAST discovery and Test262 inputs are
absent from this checkout. A retained revision still needs the all-eight
fixed-work layout guard and eventually a stock-harness qualification campaign.


## Revised-source all-eight guard — rejected on M4

Raw report: [`all-eight-fixed-work-11-revised-m4.json`](all-eight-fixed-work-11-revised-m4.json). Guard classification checkpoint: [`all-eight-guard-checkpoint-revised-m4.json`](all-eight-guard-checkpoint-revised-m4.json). This is the revised source at `ac928099b795952804f65abc8439268694f25ea5`, measured on the Apple M4 arm64 host with 11 fixed-work pairs per fixture; source was clean and stdout matched.

The layout candidate is rejected for M4 integration because two fixtures have clean cycle regressions whose bootstrap intervals exclude zero:

- DeltaBlue: 9/11 clean; cycles +1.886% (95% interval +0.810% to +5.188%); instructions +0.973% (above the 0.14% A/A band); max RSS −2.141%.
- Richards: 11/11 clean; cycles +1.513% (95% interval +0.960% to +1.861%); instructions +0.049%; max RSS −0.898%.

Other results: Crypto 11/11 clean, cycles +0.023%, instructions +0.136% (within the 0.14% band), RSS −0.601%; Splay 9/11 clean, cycles −2.125% (95% interval −4.534% to −0.676%), instructions +0.067%, RSS −20.745%. EarleyBoyer, NavierStokes, RayTrace and RegExp had 0/11, 3/11, 0/11 and 0/11 clean pairs respectively; their cycle outcomes remain inconclusive, and instruction changes exceeded the A/A band. No top-up was run because the clean DeltaBlue and Richards regressions already reject the candidate under the layout gate.

The revised Splay-only result remains a strong Splay trade-off, not a keep: fixed-work proxy distances are 1.203× speed and 1.654× RSS versus the previous M4 bars, and the candidate has not won either bar. The pushed candidate ref remains available for the Linux lane's own-host experiment, but this M4 layout must not merge to trunk in its current form.
