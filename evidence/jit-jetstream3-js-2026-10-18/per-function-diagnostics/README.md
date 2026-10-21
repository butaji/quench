# JavaScript JIT diagnostic screen

This is attribution data for the JS tier, not score qualification. The broad
screen ran one round for 27 selected JetStream 3 JavaScript fixtures with
`--jit --jit-diagnostics`; 25 validated with equal output. The two JSON
inspector fixtures could not run because the pinned checkout lacks
`SeaMonster/inspector-json-payload.js`. The report is a dirty-tree subset and
is not comparable to the clean, 11-round qualification protocol.

The binary used for the broad screen predates source-file labels in the
counters. A rebuilt, two-fixture diagnostic then confirmed the compiled
candidates by source path. `plusOne(value) { return value + 1 }` also passed a
direct semantic smoke covering numeric calls, string fallback, BigInt/Number
TypeError, and exactly-once object coercion; its diagnostics recorded 40
numeric entries, one promotion, one successful compile, 9 native entries,
and a 4 KiB mapping.

Across the 27-fixture screen, 1,553 per-function rows contained four compiled
functions: three in `validatorjs` and one in `postcss-wtb`. The validatorjs
functions each reached roughly 7.6k–22.5k entries and entered native code
7.6k–22.5k times, with 4 KiB mapped per function. The postcss-wtb function
compiled after 45 entries and reached native code 14 times. Twenty-three
valid fixtures had no native JS entries. The one-round score and RSS samples
are retained in the JSON but do not show a paired JIT-versus-interpreter or
peer win.

The strongest rejected candidates were `hash-map`'s `equals` (54 million
entries), `sync-fs`'s `get data` (36.95 million), `async-fs`'s `get data` (24.68
million), and PostCSS's `get proxyOf` (11.39 million). Their compile attempts
were rejected by the current narrow compiler. The screen does not record the
rejection reason, so this is a target list rather than evidence that any one
extension will improve score.

The source-labelled rerun used the rebuilt binary on `validatorjs` and
`postcss-wtb`. It confirms that the three validatorjs native functions and
PostCSS's one low-entry native function come from the benchmark bundles; most
promoted PostCSS candidates remain unsupported. Both reports preserve the
stock JetStream driver and output validation.

Commands used:

```sh
target/iteration/quench-bench --fixtures web-ssr,typescript-lib,babel-wtb,babylon-wtb,acorn-wtb,espree-wtb,esprima-next-wtb,prettier-wtb,postcss-wtb,source-map-wtb,babel-minify-wtb,chai-wtb,doxbee-promise,doxbee-async,async-fs,sync-fs,js-tokens,lazy-collections,json-parse-inspector,json-stringify-inspector,validatorjs,UniPoker,hash-map,stanford-crypto-aes,stanford-crypto-pbkdf2,stanford-crypto-sha256,bigint-noble-ed25519 \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/iteration/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out js-screen.json

target/iteration/quench-bench --fixtures validatorjs,postcss-wtb \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench /workspace/quench/target/iteration/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out named-candidates.json
```
