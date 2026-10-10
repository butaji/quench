# Splay compact-object profile

This is internal heap accounting for the compact-object candidate on the merged
trunk. It is not a performance gate. The profile binary adds per-record
instrumentation, so its process RSS and cycles are excluded from candidate
comparisons.

The candidate binary was built from base `a03e0646f` plus the working-tree
diff recorded in `summary.json`; its SHA-256 is
`fb87805f37b355daceea42083b1be33d44639b4657da72760143a97e73ac76bc`. The
baseline profile and exact 300-run materialized input are from
[`task61-splay-step0-post-merge-2026-10-10`](../task61-splay-step0-post-merge-2026-10-10/README.md).

At the matching pre-GC high-water phase, the candidate had 1,030,517 compact
object records and 496,400 legacy array cells. Its `Cell`/`Slot` stride is 40 B,
including for arrays, because the two inline property values moved out of the
uniform `Cell` layout. Compact-object reserve plus legacy slot reserve was
62,586,880 B, down 24,920,064 B from the baseline slot reserve of 87,506,944 B.
The candidate's internal accounted runtime bytes were 76,428,506 B versus
94,828,990 B in the baseline profile. The source change affects both record
routing and the global cell stride, so these totals are not a measurement of
only the typed-object-space benefit.

Raw candidate output: [`profile.stdout`](profile.stdout) and
[`profile.stderr`](profile.stderr). Machine-readable matching peak values:
[`summary.json`](summary.json).

## Fixed-work guard: rejected

The candidate was compared with the pinned merged-trunk baseline using the
11-round all-eight fixed-work guard. Six fixtures had 11/11 clean pairs,
Richards had 7/11, and Splay had 0/11 after the host became contended. On the
clean fixtures, candidate instructions rose by 1.65% to 29.99%, including
12.23% on DeltaBlue, 10.99% on RayTrace, and 29.52% on Richards. Their clean
cycle medians rose by 0.96% to 26.66%. Splay instructions rose 7.42%; its
cycle result is unavailable because no pair passed the contention detector.

The candidate's fixed-work RSS median fell 19.13% on Splay, from 120,815,616 B
to 96,829,440 B. This is diagnostic only: the candidate fails the all-eight
speed guard and does not change the active Splay distances (1.24x speed,
2.09x RSS). The complete raw report and resumable guard decision are in
[`all-eight-fixed-work-11.json`](all-eight-fixed-work-11.json),
[`fixed-work-guard.json`](fixed-work-guard.json), and
[`fixed-work-guard.decision.json`](fixed-work-guard.decision.json).

## Revised ArrayRecord budget

The compact-object candidate also reduces the uniform legacy `Cell`/`Slot`
stride from 56 B to 40 B because inline property values leave the enum. Arrays
therefore already benefit from this global stride reduction. An `ArrayRecord`
of 16 B would save 24 B per reserved array record relative to this candidate,
not 40 B. At the observed 496,400 live arrays, that is about 11.9 MB before
accounting for the separately rounded array-space capacity and the legacy
space capacity released by moving arrays. The earlier 19.9 MB estimate used
the 56 B trunk stride and overstated the incremental ArrayRecord benefit.
Implementing ArrayRecord remains deferred until the compact-object speed
regression has a measured explanation and a passing guard.

## Legacy-index decode follow-up: neutral

A second Splay-only 11-round diagnostic compared the rejected compact-object
binary with a follow-up that made `Value::heap_index` check the heap tag and
legacy-space bits directly, avoiding the general `HeapRef` decode. All 11
pairs were contention-clean. Marginal instructions changed by +0.07% and
marginal cycles by −0.07%, so this does not explain or recover the compact
candidate's broader regression. The source-only fast path was reverted; the
report is [`heap-index-followup-splay-11.json`](heap-index-followup-splay-11.json).
