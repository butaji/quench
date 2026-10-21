# TOD-165: share repeated short strings within one JSON.parse — rejected

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`.

- Base `b165`: v2-gc with TOD-164 step 1.
- Candidate `c165b`: branch `cand-gc-json-strings` (8c26a93b9).

## Change

`JsonHeapBuilder` kept a per-parse map from content hash to string `Value` for strings of
at most 64 units, up to 16384 entries. A hit reused the value after an equality check. A
collection epoch change cleared the map.

## Result: rejected (rule: Score up on V8-v7 or JetStream, no suite below ×0.99, RSS ≤ ×1.005 per suite, npm ≤ ×1.01)

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 5 | ×1.000 | ×1.008 |
| JetStream (6 tests) | 5 | ×0.973 (doxbee-promise ×0.893, 0/5) | ×1.002 |
| Frameworks | 5 | ×1.007 | ×1.032 |
| npm top 300 | 1 | — | ×1.0028, 5 packages worse by 5% |

json-parse-inspector peak RSS did not move (84.7 → 84.7 MB). The benchmark parses the
same payload in each of 20 iterations, and the dedupe census counted duplicates across the
whole heap. Most of the 2.8 MB of duplicates probably come from different iterations, which
a per-parse cache cannot share. The peak is also likely set by the payload source and the
decompression rather than by the parsed strings. This was not separated further.
No further work is queued on this.

The unit tests did not run because the disk filled during the debug build. That does not
change the decision.
