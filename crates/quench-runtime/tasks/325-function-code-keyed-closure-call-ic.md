# 325 — Function-code-keyed closure call IC experiment

Status: complete

Test the smallest independent slice of Task 35 before changing the native call-frame
boundary. A `CallIcSite` starts with an exact tagged-function key. On a miss by a distinct
closure whose immutable OXC function-body identity matches, it transitions to a
function-code key and derives the environment from the current closure. The IC state is a
named discriminated union; the non-null immutable call recipe remains the sole publication
fact. No runtime hotness counter or benchmark identity participates.

The experiment implemented the two key modes, added separate exact/closure hit and
transition counters, and proved with a focused test that two closures from the same body
share compiled code while still returning values from their distinct captured
environments. All 116 release tests passed.

## Result: rejected from the runtime

The V8v7 workload does not materially exercise this transition. With direct-call-region
diagnostics enabled for a short run, Richards, RayTrace, and Splay recorded zero closure
hits and zero transitions. DeltaBlue recorded only five closure hits and one transition.
The four suites together recorded millions of exact-value hits. This directly disproves
the hypothesis that same-body/different-environment closures are a significant current
call-IC miss source.

An alternating three-repetition complete-suite comparison measured:

- Richards: 787 -> 784 (-0.38%)
- DeltaBlue: 743 -> 736 (-0.94%)
- Crypto: 1621 -> 1668 (+2.90%)
- RayTrace: 1788 -> 1782 (-0.34%)
- Earley Boyer: 2473 -> 2447 (-1.05%)
- RegExp: 3484 -> 3461 (-0.66%)
- Splay: 3445 -> 3405 (-1.16%)
- Navier-Stokes: 6076 -> 6144 (+1.12%)
- geometric mean: 2044.81 -> 2043.33 (-0.07%)

This is neutral noise, not an improvement. The implementation was removed so every call
site does not retain two extra key fields and an additional state branch for an almost
unused mode. The focused result remains design evidence for Task 35: function-code closure
mode is semantically sound, but should only return together with the in-place native call
and continuation work of Task 181, where it can eliminate the actual Rust call boundary.

Artifacts: `reports/task325-closure-code-call-ic-ab-3/`.

Acceptance for this bounded experiment: correctness test passes, transition frequency is
measured, complete V8v7 A/B is recorded, and non-improving runtime overhead is retired.

