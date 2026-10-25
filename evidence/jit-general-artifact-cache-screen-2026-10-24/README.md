# GeneralDispatch dynamic artifact cache screen

This is a three-round, two-fixture production screen of the VM-local idle cache for GeneralDispatch artifacts. Both arms use the same production binary, built from dirty source at `1edcc2859` with Rust 1.99.0. Binary SHA-256: `37d38d393d216afd85fba82c26777821fe454348900eafda602a380815c6122d`. The benchmark runner's `host.rustc` field is empty, but the build shell selected Rust 1.99.0.

The only arm difference is `QUENCH_JS_JIT_GENERAL_ARTIFACT_CACHE`: 0 for off, 1 for on. `quench-bench` clears inherited environment, so each report uses a small wrapper that sets the flag and executes the same binary. Both use `--jit --jit-js-general-dispatch`, Node v26.12.0, and Bun 1.4.4. The runs were sequential (off, then on), not interleaved. All fixture outputs validated and matched across engines. The report marks `qualification_ready=false`.

| Fixture | Quench Score off → on | Score ratio | Quench max RSS off → on | RSS ratio |
| -- | --: | --: | --: | --: |
| Web-SSR | 14.00 → 14.70 | 1.050x | 116.43 → 117.05 MiB | 1.005x |
| hash-map | 7.77 → 8.87 | 1.142x | 46.95 → 46.98 MiB | 1.001x |

The short sequential screen is noisy: Node and Bun also moved substantially between arms, and hash-map's Quench rounds ranged from 4.92 to 9.31 with cache on. Do not treat these medians as a causal score improvement or qualification. The cache-on arm slightly increased peak RSS in both fixtures, while remaining far below the peers on RSS. It did not meet the ≥1.20x score target.

A separate unoptimized one-round Web-SSR diagnostic on the same cache-on source verified output equality and recorded 29 cache hits over 30 compiled `Ru` eval copies. It emitted 176,992 GeneralDispatch code bytes. An earlier diagnostic using the first, overbroad key produced no eligible cache keys and 1,627,456 emitted bytes; that comparison only motivated narrowing the key to constants and name atoms referenced by the function. These diagnostic code-byte counts are not score evidence.

Reproduction (the wrappers used in this screen pointed at `target/quench-node-artifact-cache`):

```sh
target/production/quench-bench --fixtures web-ssr,hash-map --suite jetstream3 --jit --jit-js-general-dispatch --runs 3 --timeout-ms 600000 --quench target/quench-node-cache-off --node /workspace/node-v26.12.0-linux-x64/bin/node --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true --out cache-off.json

target/production/quench-bench --fixtures web-ssr,hash-map --suite jetstream3 --jit --jit-js-general-dispatch --runs 3 --timeout-ms 600000 --quench target/quench-node-cache-on --node /workspace/node-v26.12.0-linux-x64/bin/node --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true --out cache-on.json
```

Raw reports: [cache-off.json](cache-off.json), [cache-on.json](cache-on.json).
