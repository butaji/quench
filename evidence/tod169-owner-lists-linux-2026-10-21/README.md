# TOD-169: list property-arena owners only for a compacting full collection

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`, both binaries run with
`QUENCH_COMPILE_CACHE=`.

- Base `b168`: v2-gc at 0299d8ee2.
- Candidate `c169`: `cand-gc-owner-lists`.

## Change

Every full sweep listed each live arena owner, 8 bytes per slot owner and 16 bytes per
record owner, pre-sized from the previous counts. Compaction then usually returned early
because it was not due.

Now a full collection decides before its sweep whether compaction is due
(`compacts = full && compaction_due()`), and only a compacting one lists owners. A
compaction that the sweep's own releases make due waits for the next full collection.

## Result: kept

Rule, written on TOD-169 before measuring:
- No suite's RSS above ×1.005 and no suite's speed below ×0.99.
- At least one of: npm ≤ ×0.999, or large-heap JetStream RSS down.
- No new npm failures; unit tests pass.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | ×1.023 | ×1.002 |
| JetStream (11 tests, including typescript-lib and babel) | 3 | ×1.031 | ×0.996 (typescript-lib 226.7 → 223.1 MB) |
| Frameworks | 3 | ×1.072 | ×0.999 |
| npm top 300 | 1 | — | ×0.9995; execa fails on both binaries |

quench-runtime lib: 791/791.

The suite speed gains are larger than the change explains and are likely mostly noise.
The memory effect is the typescript-lib peak.
