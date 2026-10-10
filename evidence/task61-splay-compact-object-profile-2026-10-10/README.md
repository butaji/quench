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

## Richards attribution of the speed regression

The all-eight guard showed a clean +29.52% instruction delta on Richards for
the compact-object candidate. Richards is useful here because it exercises
the shared property path with a small heap, so full-process Callgrind fits in
the aarch64 Linux VM. These profiles are attribution evidence only; the M4
gate remains authoritative.

The two binaries share base commit `a03e0646ff216a192d85c6d1d22307855cdb21f2`.
The baseline executable SHA-256 is
`ec65a4e93b7db01de2acbf0b67a1ec357a21ae33472153a23615961834b5e326`; the
candidate executable SHA-256 is
`e3321d6d06cac1552f15f9e45d2d36e87ed693b9ef8cff4b408da22af1d115fc`, built
with candidate source diff SHA-256
`a9597d2046b32a98a99eda0f0fa1a1221ccdbb3c8fc76d89c31454504826200a`.
The harness inputs are `base.js` + `richards.js` + the same fixed-work runner,
with `Setup`, K calls to `run()`, then `TearDown`; K is 0 or 1. Both inputs
validate and print their expected marker. Input hashes and raw profile totals
are in [`callgrind-richards-attribution.json`](callgrind-richards-attribution.json).
Compressed raw profiles and exact inputs are retained beside it. The parser
resolves function names defined by either `fn=` or `cfn=`, excludes inclusive
call-edge costs from self counts, and asserts that self-Ir sums to each file's
summary before writing the report.

On Rust 1.99.0 / Debian Bookworm aarch64 with Valgrind 3.19.0, baseline
K=1−K=0 was 145,490,912 Ir; candidate K=1−K=0 was 190,143,949 Ir (+30.69%).
The largest candidate-minus-baseline self-Ir changes were:

| Function | Self-Ir delta |
| --- | ---: |
| `Heap::object` | +22,066,060 |
| `Vm::object_property_slot` | +8,775,013 |
| `Vm::get_field_miss` | +6,721,403 |
| `Vm::immediate_prototype_data_field` | +6,211,764 |
| `Vm::shape_property_lookup` | +4,294,081 |
| `Vm::shape_attribute` | +3,996,865 |
| `Heap::object_mut` | +2,031,400 |

Callgraph counts attribute 555,311 candidate calls to `Heap::object` per
Richards run (39.7 self-Ir/call) and 50,785 calls to `Heap::object_mut`
(40.0 self-Ir/call). The candidate's `Heap::object` first decodes a `HeapRef`
and branches on its space; its legacy branch then calls `get(value)`, which
decodes the same `Value` again, and wraps the result in `ObjectRef`. This
space/view path is new relative to the baseline's direct `heap.get(value)` +
`Cell::object()` path. The accompanying increases in field lookup functions
show that the regression is distributed across the shared property path, not
compact-object allocation. This confirms the M4 regression signal and gives a
specific path to remove before re-gating the candidate.

## Legacy-index decode follow-up: neutral

A second Splay-only 11-round diagnostic compared the rejected compact-object
binary with a follow-up that made `Value::heap_index` check the heap tag and
legacy-space bits directly, avoiding the general `HeapRef` decode. All 11
pairs were contention-clean. Marginal instructions changed by +0.07% and
marginal cycles by −0.07%, so this does not explain or recover the compact
candidate's broader regression. The source-only fast path was reverted; the
report is [`heap-index-followup-splay-11.json`](heap-index-followup-splay-11.json).

## Richards legacy-slot reuse follow-up: regression remains

The first narrow follow-up reused the `HeapRef` index in `Heap::object` and
`Heap::object_mut` instead of decoding the `Value` again through `get`/
`get_mut`. A fresh Richards fixed-work comparison against the pinned trunk
baseline completed with 11/11 clean pairs, valid output and identical results.
The candidate still regressed marginal instructions by 29.99% and cycles by
28.32%: 194,700,642 versus 149,782,816 instructions per run, and 29,909,853
versus 23,309,261 cycles per run. Work RSS was 19,415,040 B versus
20,054,016 B. Thus reusing the decoded index does not account for the shared
property-path regression; this candidate remains rejected and the active Splay
distances stay at 1.24x speed / 2.09x RSS.

The report is [`richards-handle-reuse-11.json`](richards-handle-reuse-11.json).
It records the exact M4 paired samples and executable hashes: trunk baseline
`127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`,
candidate `30c7eccb286aac96d9d46fb4cad8803ebac04e73629478e5a49ef3f36f9c599d`.
The runner's `source_revision` is the clean trunk checkout (`a352d170f`); the
candidate binary was built from compact-space worktree base
`a03e0646ff216a192d85c6d1d22307855cdb21f2` plus dirty diff SHA-256
`c5e61c1173015c93f785b7a7b9b9932c4248420432b7e53a3be71385a4ad85dd`.
The report's source revision therefore identifies the runner checkout, not the
candidate build source. Candidate compilation and 546 runtime library tests
passed before pinning.
