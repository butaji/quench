# TOD-164 step 1: minor collections skip survivors in the record sweeps

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`.

- Base `c162`: v2-gc production.
- Candidate `c164`: this change.

## Change

- `RecordSpace::sweep` takes `visit_live`.
- The live-record visit does work only on full collections: property-owner lists and the
  payload recount.
- On a minor collection the sweep visits only dead records and counts survivors with a
  popcount of each mark word.
- Await states never visit live records.

## Result: kept (TOD-164 rule: Score up on V8-v7 or JetStream, no suite below ×0.99, RSS ≤ ×1.005 per suite, npm ≤ ×1.01)

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 5 | ×1.012 | ×0.999 |
| JetStream (10 GC-heavy tests) | 5 | ×1.018 (lazy-collections ×1.033 and acorn ×1.016, both 5/5) | ×1.000 |
| Frameworks | 5 | ×1.021 | ×1.001 |
| npm top 300 | 1 | — | ×1.0036, none worse by 5%, statuses unchanged |

quench-runtime lib: 776/776.
