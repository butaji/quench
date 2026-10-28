# Function-source cache on Linux

This is a focused Stage B candidate, not Task 61 qualification.

## Change

Closures from the same compiled function ID now reuse a program-rooted source
string `Value`. Each closure object, environment, function identity, `name`,
`length`, and `.prototype` remain freshly constructed as before. The shared
value is an immutable primitive string used by the existing function source
path. Node v24.19.0 and Quench produced byte-for-byte equal oracle JSON for
fresh closure identity, `Function.prototype.toString()`, and calls through two
closures. See `node-oracle.cjs`, `node-output.json`, and `quench-output.json`.

ECMAScript specifies the source-text result through
[Function.prototype.toString](https://tc39.es/ecma262/#sec-function.prototype.tostring);
Node is the runtime oracle for the concrete closure behavior.

## Eleven-pair EarleyBoyer result

Eleven alternating production pairs on the materialized V8-v7 fixture showed:

- Median Score: 188 to 208 (+10.64%; paired 95% interval for the median delta
  +14 to +24 points).
- Median maximum RSS: 52,916,224 to 50,724,864 bytes (-2,191,360; paired 95%
  interval -2,588,672 to -1,839,104 bytes).
- Candidate RSS was lower in all 11 pairs, candidate Score was lower in 0/11,
  and output matched in all pairs.

The report and ordered pair log are `earley-pairs.json` and
`earley-pairs.jsonl`. Binary hashes, fixture hashes, host, environment, and the
runner hash are recorded there.

## Other-fixture controls

A one-round all-eight screen was favorable on EarleyBoyer, RegExp and Splay
Score, and lower RSS on seven fixtures; Crypto's RSS was 577,536 bytes higher.
Those samples are directional only. A subsequent three-pair screen on the
seven non-Earley controls found Score lower on every Crypto and NavierStokes
pair, with median deltas -2 and -6 points. DeltaBlue's Score interval included
a tie. RayTrace, RegExp, Richards, and Splay Score intervals also included a
tie. RSS was lower on every pair for Crypto, DeltaBlue, NavierStokes, and
RayTrace; intervals included a tie for RegExp and Richards; Splay RSS was
slightly higher in all three pairs. The complete per-fixture intervals and raw
samples are in `controls-screen.json` and `controls-screen.jsonl`.

The all-eight one-round results and raw rows are in `all8-screen.json` and
`all8-screen.jsonl`. This mixed control evidence means the cache remains a
focused EarleyBoyer pre-gate candidate and is not claimed as a general
all-fixture Score win.

## Correctness

The runtime library suite passed 521/521. Selected Test262 results were:

- Stage 8: 347/347.
- Stage 10: 11,087/11,102; its 15 failures are the already documented
  compound-assignment and prefix/postfix cases.
- Stage 11: 217/217.
- Stage 24: 1/1.
- Stages 48–50: 593/593.

Full reports are included as `test262-*.json`. The implementation patch is
`source.patch.gz`; its uncompressed SHA-256 is recorded in the report.
