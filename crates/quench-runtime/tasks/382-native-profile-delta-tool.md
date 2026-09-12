# 382 — Native-profile delta tool for before/after candidate comparison

Status: complete

**The one concrete tooling gap found by inventorying `scripts/`.** This project has
mature statistical acceptance tooling (`v8v7-paired-stats.py`, `test-v8v7-exact-driver.sh`
for the nine-pair exact gate) and mature candidate-selection tooling
([[377]]'s `residual-frontier.py`, cost-scored against dynamic entries and physical
tile cost). What's missing is the fast middle step: once a candidate is implemented,
comparing its *actual* native profile against the pre-change baseline symbol-by-symbol
to see exactly which functions shrank, which grew, and whether any new seam-crossing
symbol appeared — today this appears to be done by eyeballing two separate
`.sample.txt`/normalized-TSV files side by side (per [[253]]'s
`normalize-macos-sample.sh`/`normalize-v8-prof.sh` output format), which is slow and
error-prone specifically for the failure mode that has burned real effort twice already
(task 362's added native/Rust/native seams, task 381's reverted bitwise/numeric-unary
catalog) — a seam that shows up as a *new* or *grown* symbol in the post-change profile
that a fast diff would surface immediately, where a full nine-pair exact-gate cycle
currently has to run to completion before the regression is even visible.

Concrete steps:
1. Write `scripts/profile-delta.py` (or `.sh`) taking two normalized profile TSVs (the
   same format [[253]]'s `normalize-macos-sample.sh`/`normalize-v8-prof.sh` already
   produce) and emitting a delta table: symbol, before%, after%, absolute change, sorted
   by magnitude of change — surfacing shrunk symbols (the intended win), grown symbols
   (a possible regression), and new symbols (a likely added seam) in one view.
2. Wire it as an optional step in `scripts/perf-cycle.sh`'s existing `compare`/`profile`
   operations, so a candidate A/B run can optionally emit this delta alongside the
   score comparison, not as a separate manual step.
3. Retroactively run it against at least one already-recorded before/after pair from a
   rejected candidate (task 362's or task 381's `.sample.txt` artifacts, if preserved)
   to confirm the tool actually would have surfaced the seam/regression faster than the
   full exact-gate cycle did, as a concrete validation of the tool's value rather than
   an assumed one.

Acceptance: `profile-delta.py` (or equivalent) produces a sorted before/after delta table
from two existing normalized profile artifacts; it is wired into [[00]]'s existing
per-change routine as an optional fast-feedback step, documented alongside the existing
`perf-cycle.sh` operations; the retroactive validation against a real rejected
candidate's artifacts confirms the tool would have surfaced the regression signal before
the full statistical gate completed, or (if it would not have) that finding is recorded
honestly rather than the tool being adopted on faith.

No external primary source needed — this fills a gap between two already-existing,
already-cited tools ([[253]]'s normalizers, [[00]]'s exact-gate driver) rather than
introducing a new methodology.

## Implementation and validation

`scripts/profile-delta.py` now reads normalized native-profile TSVs by column name,
canonicalizes away the profiled binary image suffix, aggregates duplicate
`(suite, symbol)` rows, and emits before/after/delta/status rows sorted by absolute
change. All limits and column identities are named constants. Four unit tests cover
ordering/status, duplicate aggregation, different baseline/candidate image names, and
invalid input.

`scripts/perf-cycle.sh profile-pair` profiles two binaries under the same suite/window,
normalizes both samples, and writes `profile-delta.tsv`. The ordinary `compare` command
also emits that file when `DEEGEN_BASELINE_PROFILE_TSV` and
`DEEGEN_CANDIDATE_PROFILE_TSV` are supplied. `profile-delta` exposes the pure comparison
for already-recorded artifacts.

The retroactive check used the rejected Task 384 copy-safe-property candidate against
the accepted Task 381 binary on Crypto. Artifacts are in
`reports/task382-profile-delta-validation/`. The result is deliberately negative: the
samples attribute 85.83% versus 86.09% to the same general block executor and show only
small symbol changes, so this profile pair would **not** have exposed the large short-run
score regression by itself. This validates the tool's mechanics and its limitation:
top-of-stack native sampling cannot explain time that moves between generated anonymous
stencils or changes the number of benchmark iterations. It remains a fast seam detector,
not a substitute for counters or the paired score gate.
