# Boolean endianness check in the DataView JIT lane

The Float64 DataView JIT lane now checks whether the endianness argument is a
Boolean before using the general `ToBoolean` implementation. Non-Boolean
values still use the original truthiness path. The change applies only in
compiled mode and preserves the interpreter path and all DataView fallback
cases.

## Evidence

Both three-round runs and all seven interleaved one-round A/B pairs passed
JetStream output validation. The clean baseline is source revision
`df846aa7ddfc4f81a38dae17236c91d5ff84d1b6`, with executable SHA-256
`059caf3a9271744d2b23db228d9aa74751441a0c779a0065d0687050ccd0f158`. The
candidate executable SHA-256 is
`2a7d34b581952756eddc79533a4ff6df11ab76a15c8fa1a7e130f5941a66fe7d`.

The three-round control scored 3.80 / 87.75 MiB; the candidate scored 3.94 /
88.04 MiB. Bun's median moved from 144.65 to 153.43 and Node's from 81.36 to
74.39, so those separate windows are not a reliable paired peer comparison.

Across seven one-round Quench pairs, six favored the candidate. The median
candidate/control Score ratio was 1.083 and the geometric mean was 1.106. The
median peak-RSS ratio was 1.0014 (about 0.12 MiB higher at this fixture's
baseline). Two pairs ran candidate first to check order bias; both favored the
candidate (4.26 vs 3.02 and 3.84 vs 3.71). Bun and Node scores varied sharply,
including opposite movement within some pairs, so peer-normalized ratios are
not treated as evidence for a peer win. This is a local async-fs optimization
screen, not JetStream qualification or a win against Bun or Node.

Raw reports for the clean baseline, candidate, and all seven pairs are in this
directory. Pair reports record the source tree as dirty because the candidate
was screened before its source change was committed; every report records the
executable hash and whether JetStream validation passed.

## Reproduction

```sh
cargo build --locked --profile production -p quench-node -p quench-bench

target/production/quench-bench async-fs --suite jetstream3 --jit --runs 3 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node node --bun /path/to/bun \
  --out candidate-3round.json

# A/B pairs use the same runner and JIT peers, changing only the Quench binary.
target/production/quench-bench async-fs --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench /path/to/baseline-quench-node \
  --node node --bun /path/to/bun --out pair-off.json

target/production/quench-bench async-fs --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node node --bun /path/to/bun --out pair-on.json
```
