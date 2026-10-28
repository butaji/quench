# V8-v7 JIT coverage after OSR merge

Coverage-only diagnostic for the merged loop OSR tier; this is not a JetStream score comparison.

- Source revision: `5a53f8903d9558609fbf3070206dd79d256de4da` (`v2-jit`, after merging the then-current `origin/v2-merged`; that remote ref has since disappeared, and `origin/v2-jit` is an ancestor).
- Production `quench-node` SHA-256: `383cd816ba60f6395f6519a1cc6d6a3d35ced9aa6507e98248a4f6f06a212eda`.
- Inputs: stock V8-v7 scripts from the pinned corpus in `../v8-v7-native-coverage/`.
- Command: `QUENCH_JS_JIT_STATS=1 target/production/quench-node <script>` for Richards, DeltaBlue, NavierStokes and Crypto. All four completed their own benchmark assertions. Printed benchmark Scores are excluded as performance evidence.

| Fixture | Native function entries | OSR admitted sites | OSR compile successes | Native region runs |
| --- | ---: | ---: | ---: | ---: |
| Richards | 0 | 0 | 0 | 0 |
| DeltaBlue | 17,539 | 0 | 0 | 0 |
| NavierStokes | 0 | 22 | 26 | 17,435 |
| Crypto | 2,456 | 12 | 8 | 6,120,293 |

The OSR tier executes numeric regions, while Richards' hot loops remain outside native coverage because they combine object fields, calls, construction, and writes. These counters identify coverage gaps; they do not prove score or memory gains. Raw stdout and stderr are retained alongside this report.
