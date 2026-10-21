# TOD-163: trim property-arena class slack at compaction (held, not adopted)

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`.

- Base `c162`: v2-gc with string records (TOD-162).
- Candidate `c163`: this branch.

## Motivation

typescript-lib census (profile-memory, accounted peak 152 MB):

- The property arena holds 66 MB.
- Live vectors use 5.11M values but hold 6.69M capacity, about 12 MB of power-of-two slack.

## Change

- Compaction copies each live vector to exactly its length and records the start in an
  exact-capacity bitmap.
- The next growth returns the vector to its power-of-two class.
- Released exact ranges wait for compaction instead of entering the size-class free lists.

## Decision rule (TOD-163)

- No suite's RSS geomean higher, and lower on JetStream or npm top 300.
- Score geomean at least ×0.99 per suite.

## Result: rule not met; held on branch `v2-gc-tod163`

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 5 | ×1.020 | ×0.990 |
| Frameworks | 5 | ×1.050 | ×0.998 |
| JetStream (13 property-heavy tests) | 3 | ×1.027 | ×0.997 |
| npm top 300 | 1 | — | ×1.0014, none worse by 5% |

- npm RSS is ×1.0014 (higher), so the rule fails.
- Every RSS change is within noise.
- typescript-lib, where the slack was measured, fails on the current base (upstream regression,
  TOD-133), so the expected about 12 MB could not be checked.
- **Revisit when typescript-lib runs again.** Also check how often compaction triggers on these
  workloads: it runs only once released ranges reach a quarter of the arena.
- quench-runtime lib passes 777/777, including a new unit test for exact capacity.
