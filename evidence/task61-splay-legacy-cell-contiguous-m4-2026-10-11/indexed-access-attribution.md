# Indexed-access attribution — M4

`profile-aggregate` K=0/K=300 runs on Crypto and NavierStokes. These are
deterministic counters, not timing measurements.

## Inputs and scope

The work profiles use the trunk instrumented binary
`96b98b605ef0a77b29282c5decb088c3b14f61d53da6143474c50ba2170ae63e` and the
pre-contiguous direct-object candidate instrumented binary
`ad6d9eef2c4470bb2dd48f653babd4c4c2d237cb306eeb60030966113561f098`. The
production timing comparison is the newer contiguous-cell binary
`bdcc9a93be82d25c1bdef0bbc1c7db677a0dd4905b87d92a8b0c6d341f87ca71` versus
trunk `ceeca73f772b141ee7619ae869bffaac1d93eeee99ecba2b07337d8345937e80`.

The indexed-access counter sites and `vm/index.rs` behavior are unchanged by
the later contiguous-cell patch; the candidate diff there is formatting only.
Both instrumented binaries report identical marginal indexed-access counts.
The current contiguous-cell candidate itself was not rebuilt with profiling:
the profiling binary reports operation counts, while its instructions below
come from the exact production screen binaries.

Crypto has two benchmark functions, so the averages divide K=300 work by 600
calls; NavierStokes has one benchmark function, so its divisor is 300.

## Counts and whole-fixture instruction deltas

| Fixture | Dense gets/run | SetIndex dispatches/run | Total indexed dispatches/run | Production Δ instructions/run | Δ / indexed dispatch (upper bound) |
|---|---:|---:|---:|---:|---:|
| Crypto | 1,215,736 | 433,442 | 1,649,177 | +113,359,696 (+7.70%) | 68.8 |
| NavierStokes | 3,426,760 | 1,013,777 | 4,440,537 | +223,692,771 (+7.29%) | 50.4 |

The last column assigns the entire fixture instruction delta to indexed
dispatches. It is an upper bound on their contribution, not a measured
per-opcode cost: calls, arithmetic, allocation, and other changed paths also
contribute. The counts confirm these are index-heavy fixtures and that the
candidate did not execute more indexed operations. They do not by themselves
prove that array access explains the full instruction delta.

## Reproduction

The four materialized inputs are `indexed-crypto-{0,300}.js` and
`indexed-navier-stokes-{0,300}.js`. Run each input with both instrumented
binaries and read the `quench-profile` JSON line on stderr. The raw profile
objects are retained as `raw-*.json`; K=300 minus K=0 is summarized in
`indexed-access-crypto-navierstokes-k300.json`.

The production instruction deltas are from `crypto-screen-11.json` and
`navierstokes-screen-11.json` in this directory. Those screens originally
reported 11/11 clean pairs under schema 4. Offline schema-5 reclassification
leaves Crypto at 10/11 and NavierStokes at 11/11; the same policy leaves
EarleyBoyer at 9/11 and RegExp at 10/11. See
`effective-cycle-rate-audit.md` before using those intervals for a decision.
