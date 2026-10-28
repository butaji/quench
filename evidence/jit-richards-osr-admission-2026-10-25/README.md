# Richards OSR admission diagnostic

Source revision: `5b133b913ff9a03da3b3d205ce13c5d306d1da8b` (`v2-jit`), clean tree, with `origin/v2-merged` already merged through `fae9b1aa`.

This is one output-valid JetStream 3.0 Richards diagnostic using the required opt-level-0 iteration profile. It is useful for current-source coverage and rejection attribution only. The Quench score is not a production performance result and the selected one-fixture run is not qualification evidence.

| Engine | Score | Peak RSS |
| --- | ---: | ---: |
| Quench `--jit` | 1.96 | 62.1 MiB |
| Node v26.10.0 | 34.29 | 89.3 MiB |
| Bun 1.4.3 | 552.34 | 46.1 MiB |

The JS JIT row for bytecode function 37 in `/workspace/quench/quench-bench/jetstream/Octane/richards.js` had 255 entries, one backward site, zero admitted OSR sites, zero native region runs, and one `field-mutation` analysis rejection. Benchmark output was valid and equal across engines.

Reproduction from the repository root (workspace-local Node/Bun paths):

```sh
target/iteration/quench-bench --fixtures richards --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/iteration/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/quench/target/tools/bun/bun-linux-x64/bun \
  --qjs /bin/true \
  --out target/richards-iteration-current.json
```

The Quench binary was built with `CARGO_PROFILE_ITERATION_OPT_LEVEL=0 CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=2`. The report records host details, fixture hash, engine hashes, score/RSS, output parity and the full per-function JIT stats. It is a one-round, one-fixture diagnostic (`qualification_ready=false`), not a speed claim.
