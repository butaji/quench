# Splay: aligned speed-lane HEAD screen on M4

This is a fixed-work diagnostic for the active Splay benchmark. It is not a
stock-harness qualification run and does not close either Splay bar.

## Inputs and builds

- Host: M4 arm64 macOS 25.5, Mac16,10, 10 logical CPUs.
- Rust: 1.99.0, LLVM 23.1.2.
- Source candidate: `83a069a91c94fbdefde85b53680dc39f4dc6e115` from
  `origin/v2-cloud-c`, based on trunk `4a678f3967443b931e6eb7a50225a132cc5ab122`.
- Baseline binary SHA-256:
  `842d2caf1c1605f34d7d22628bd3064f4b9b399e7efdcb618776634587585ba3`.
- Candidate binary SHA-256:
  `43de900ad8553ef12db8e372a061492f160c1bff7a9af25b166b1ba7a6620509`.
- Candidate build: production profile with `-C llvm-args=-align-all-functions=6`,
  `-D warnings`, and the host's macOS 15 deployment flag; effective command is
  in `build.log`.
- Candidate `__text`: 7,404,352 B. Aligned trunk `__text`: 7,406,336 B.
- Runner: pinned `quench-bench`, SHA-256
  `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.
- Work: 11 alternating fixed-work pairs per fixture, CJS-shaped stock corpus,
  output validation on every fixture.

## Initial all-eight result

Raw report: `all-eight-fixed-work-11.json`. Every fixture produced matching
outputs. Clean-pair counts were Crypto 11/11, DeltaBlue 6/11, EarleyBoyer 0/11,
NavierStokes 8/11, RayTrace 11/11, RegExp 11/11, Richards 11/11 and Splay
11/11.

The resumable guard decision and checkpoint are `guard-checkpoint.decision.json`
and `guard-checkpoint.json`. After targeted top-ups, all fixtures are
classified and the all-eight guard passes. On clean pairs, its cycle medians
and 95% bootstrap intervals are:

| Fixture | Clean | Cycle delta | 95% interval | Instructions | Max RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Crypto | 11/11 | −7.76% | [−11.18%, −6.69%] | −11.19% | +0.30% |
| DeltaBlue | 13/18 | −3.99% | [−4.34%, −3.76%] | −5.98% | +0.31% |
| EarleyBoyer | 7/18 | −3.92% | [−4.11%, −2.97%] | −6.45% | −0.09% |
| NavierStokes | 8/11 | −6.80% | [−8.96%, −2.90%] | −11.43% | −0.23% |
| RayTrace | 11/11 | −3.05% | [−3.73%, −2.42%] | −4.35% | +0.23% |
| RegExp | 11/11 | +0.73% | [−0.96%, +2.51%] | −0.34% | −0.84% |
| Richards | 11/11 | −4.09% | [−4.80%, −3.39%] | −8.02% | −0.48% |
| Splay | 11/11 | −7.34% | [−8.04%, −6.56%] | −10.11% | −0.09% |

Splay marginal cycles/run are 5.735M → 5.307M and instructions/run are 33.070M
→ 29.724M. Versus the current M4 references (Node jitless 4.679M cycles/run,
Bun 58.696 MB max RSS), this candidate is 1.13× speed and 2.09× RSS. The
accepted trunk remains 1.24× / 2.09× until the candidate is integrated.

## Top-ups and tooling correction

DeltaBlue's initial 6/11 clean pairs and EarleyBoyer's initial 0/11 clean pairs
were topped up with separate 7-round reports; both top-ups had 7/7 clean pairs.
The guard initially rejected the DeltaBlue report because it compared the derived full-suite flag
`corpus.fixture_set_matches` (true for the initial eight-fixture report, false
for a one-fixture top-up) as though it were corpus identity. The guard now
compares pinned/checkout revisions and cleanliness independently from fixture
coverage. Its six unit tests pass, and the saved top-ups are included in the
final decision. EarleyBoyer's paired max-RSS median after top-up is −0.09%, so
same-binary RSS calibration was unnecessary.

Timing paused after host load reached 24.69 with multiple Deno processes over
100% CPU; the targeted top-ups were run after that contention cleared.

## Function-alignment attribution

The three-state × two-setting matrix is in
`../task61-splay-alignment-matrix-2026-10-11/`. On the trunk source, enabling
64-byte function alignment changed instructions by approximately zero. Clean
cycle results were neutral except Richards at +0.610% [ +0.048%, +0.932% ] and
Splay at +0.564% [ −1.555%, +2.105% ]. The aligned binary adds 214,832 B to
`__text` (+2.99%), 212,992 B to `__TEXT`, and 215,360 B to file size.

The older call-path candidate, aligned against aligned trunk, reduced Splay
cycles by 2.78%, and its earlier Crypto/NavierStokes/Richards cycle regressions
became neutral or favorable. The current screen supersedes that older candidate.
The 40 B layout still has clean aligned cycle regressions on DeltaBlue, RegExp
and Richards, so alignment alone did not make that candidate acceptable. The
aligned executable is 215,360 B larger on disk (12,271,232 B vs 12,055,872 B).
