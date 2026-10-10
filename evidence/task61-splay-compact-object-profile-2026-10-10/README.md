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

## Richards legacy GetField cache fast path: rejected

The next hypothesis kept legacy `Cell::Object` cache hits on the pre-space
property representation: it bypassed `Heap::object`/`ObjectRef` for own-field
hits and read the cached slot directly from `ValueArena`. The Richards
fixed-work gate again completed with 11/11 clean pairs and equal output, but
the path was slower: the paired marginal instruction delta was +30.66%
(195,702,313 versus 149,782,913 instructions/run; per-pair range +30.65% to
+30.67%), and the paired cycle median was +29.23% (range +26.91% to +31.47%).
Max work RSS was 19,529,728 B versus 19,972,096 B. The compact-space
regression therefore remains; this own-property-only fast path is rejected.
It does not test an inherited-holder cache fast path. Active Splay distances
remain 1.24x speed / 2.09x RSS.

Raw report: [`richards-legacy-field-cache-fastpath-11.json`](richards-legacy-field-cache-fastpath-11.json).
Candidate executable SHA-256 is
`0525552744fefc5d0ac639af2033902e28f13ecae22f77ad8ba4618611f096c6`; it was
built from worktree base `a03e0646ff216a192d85c6d1d22307855cdb21f2` plus dirty
diff SHA-256
`ce85800a2ed7946543bdc38da88029da30ea2ecc921ae45f53078b5aa77a9226`.
The report's source revision again identifies the clean runner checkout
(`a352d170f`), not that candidate worktree.

## Richards field-cache counter census: no inherited-holder hits

To classify the hot cache tier before another accessor hypothesis, a
profile-aggregate build of the compact-space candidate ran the fixed Richards
input at K=100 and K=0 on the M4 arm64 macOS host (Mac16,10, 16 GiB). Both
runner engine names used the same profile executable, so this is a counter
census only; its timings and RSS are not performance evidence. The fixed-work
plan validates at 100 benchmark iterations.

After subtracting setup-only counters, the 100-iteration work recorded
21,210,143 field-cache hits and 4,046,657 misses: about 212,101 hits and
40,467 misses per iteration. All net hits were tier 0, depth 0; there were no
depth-1 immediate-prototype cache hits. This excludes the inherited-holder
cache as a source of the Richards regression. Tier-0 counters combine own
field reads, field stores and cached field additions, so this census does not
split those operations. The earlier Callgrind attribution remains the stronger
causal evidence: the compact candidate's extra work is distributed through
`Heap::object`, `object_property_slot` and field-miss/prototype lookup paths.
Two attempts to bypass the shared object view on own-field hits were already
measured slower, so this census does not justify another version of that same
fast-path hypothesis. The active Splay distances remain 1.24x speed and 2.09x
RSS on M4 arm64 macOS.

Raw counter report: [`richards-field-cache-profile-counter-census-2026-10-10.json`](richards-field-cache-profile-counter-census-2026-10-10.json).
The profile executable SHA-256 is
`73a042380a7e70e085bc7590ce916515f4fe8be515b415f2298697e37ef19cce`; it was
built from candidate worktree base `a03e0646ff216a192d85c6d1d22307855cdb21f2`
plus profiling-only source diff SHA-256
`faf16ab6c45a3757f5d234d0432d61090985f616990dab65aaff93d407e7f718` to repair
two stale profiler-build call sites. The production candidate binaries and
their previous fixed-work results are unchanged.

## Guarded field-layout cache trial: all-eight guard rejected

The follow-up candidate keeps the compact-object layout and adds a direct
field-cache accessor guarded by the cached receiver layout. Its Splay-only
fixed-work run was 11/11 clean with equal output: max work RSS fell from
122,732,544 B to 99,368,960 B (−19.0%), while marginal cycles were +0.91% and
instructions +1.83% against the paired trunk binary. The cycle movement
overlaps the recorded Splay A/A spread. Against the existing Step 0 M4
references, its provisional fixed-work distances were 1.23x speed and 1.69x
RSS; this was not a stock-harness qualification.

The all-eight fixed-work guard then completed with at least 10 clean pairs per
fixture. It rejects this layout candidate because clean cycle intervals show
regressions on Crypto (+1.57%, 95% interval +0.51% to +2.10%), EarleyBoyer
(+1.14%, +0.58% to +1.60%) and RegExp (+2.70%, +1.50% to +4.34%). Richards
is cycle-neutral (+0.13%, interval −0.86% to +0.44%) and its field-cache
instruction count is −0.66%, confirming the direct-layout path removes the
prior Richards regression. Splay itself is cycle-neutral (+0.16%, interval
−0.91% to +0.99%) and keeps the −19.08% RSS result, but retires +1.85%
instructions. All fixtures reduce RSS; the failure is the three cycle
regressions, so this candidate is not merged.

The exact all-eight report, resumable checkpoint and classification are
[`all-eight-guarded-field-layout-11.json`](all-eight-guarded-field-layout-11.json),
[`all-eight-guarded-field-layout-checkpoint.json`](all-eight-guarded-field-layout-checkpoint.json),
and [`all-eight-guarded-field-layout-checkpoint.decision.json`](all-eight-guarded-field-layout-checkpoint.decision.json).
The guarded Splay-only run is
[`splay-layout-guarded-field-cache-11.json`](splay-layout-guarded-field-cache-11.json).
