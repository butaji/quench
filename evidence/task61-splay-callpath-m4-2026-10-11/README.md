# Splay call-path lane series: M4 integration screen

This is an M4 arm64 macOS fixed-work integration screen of the five code commits
from v2-cloud-c after `a12e54137`. The candidate is not merged: the all-eight
guard rejected it on clean cycle regressions.

## Candidate and method

- M4 integration base: `015e21345` (evidence-only trunk commit on runtime base
  `883e5c00f`).
- Lane range: `a12e54137..ba9cf8762`, cherry-picked in order as
  `47889a906`, `9d6cfe572`, `2c00c682e`, `443476932`, `849e93e3`.
- Candidate source revision: `849e93e3efe47fdb6cd315472370b54b2798d306`,
  clean; V8-v7 submodule: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Runtime tests: `quench-runtime --lib --features profile-memory`, 544/544;
  `quench-wasm`, 31/31.
- Candidate production `quench-node` SHA-256:
  `5d43cabc18d44d236374aa31db8f37d4e578106c5302436e2aabf67da8bd46e9`.
- Baseline production `quench-node` SHA-256:
  `127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`.
- Fixed-work runner SHA-256:
  `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.
- Work: all eight V8-v7 fixtures, 11 alternating pairs, with the shared
  fixed-work plan; all outputs matched. The raw report records host load and
  per-sample contention checks.
- Candidate source diff is preserved in
  [`callpath-candidate.patch.gz`](callpath-candidate.patch.gz), SHA-256
  `030ee5edf82826ef1b559dfda2a50e2257fcfa5fe5505f094af35a1e15dbfea9`.

Raw report: [`all-eight-fixed-work-11.json`](all-eight-fixed-work-11.json).
Guard checkpoint and decision:
[`all-eight-guard.json`](all-eight-guard.json),
[`all-eight-guard.decision.json`](all-eight-guard.decision.json).

## M4 result

All eight fixtures had 11/11 valid pairs. Clean cycle pairs were at least 10/11
for every fixture except Splay (4/11). The guard uses clean pairs for cycles,
all valid pairs for instructions, and valid paired maxima for RSS. Since this is
a call/dispatch change rather than a record-layout change, Splay may use the
configured instructions-only fallback while cycles are under-sampled.

| Fixture | Clean cycles | Paired cycles | Instructions | Max RSS | Guard |
| --- | ---: | ---: | ---: | ---: | --- |
| Crypto | 11/11 | +5.342% [+4.075%, +7.138%] | −0.882% | −0.302% | Regression |
| DeltaBlue | 11/11 | −1.763% [−2.080%, −0.902%] | −2.598% | −0.784% | Pass |
| EarleyBoyer | 11/11 | +0.188% [−0.173%, +0.743%] | −2.329% | −0.430% | Pass |
| NavierStokes | 11/11 | +3.777% [+2.173%, +5.012%] | −0.762% | −0.695% | Regression |
| RayTrace | 10/11 | −1.193% [−1.497%, −0.393%] | −1.196% | −0.454% | Pass |
| RegExp | 11/11 | +0.826% [−0.551%, +1.095%] | −0.074% | −0.891% | Pass |
| Richards | 11/11 | +1.346% [+1.204%, +1.860%] | −2.867% | −1.060% | Regression |
| Splay | 4/11 | −2.594% [−15.648%, +10.544%] | −3.846% | +0.495% | Instructions-only; cycles insufficient |

Crypto, NavierStokes and Richards have statistically positive cycle deltas even
though their retired instructions fell. This points to an M4 code-generation or
microarchitectural cost in the combined call-path series; it does not identify
which individual commit causes it. The combined candidate therefore fails the
all-eight M4 gate and is not merged.

The Splay instruction reduction is a real attribution signal, but four clean
cycle pairs are insufficient to update its speed distance. The active M4 trunk
remains at **1.24x speed / 2.09x RSS**. The candidate's Splay max-RSS change is
+0.495%, within the guard's +0.5% limit but at its edge. These are fixed-work
diagnostic measurements, not stock-harness qualification results.

The Linux lane's own production-CJS evidence remains recorded on Linear TOD-9:
33.25M to 31.97M marginal instructions/run (−3.9%), with wall-time and RSS
notes as reported there. The M4 result does not invalidate that Linux result;
it shows this combined series does not pass the M4 cross-fixture guard.
