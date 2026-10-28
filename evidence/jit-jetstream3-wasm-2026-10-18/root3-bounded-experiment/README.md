# Bounded HashSet root-JIT screen

This is an exploratory, one-round screen on clean source commit
`ea7c76c6c327f76f18376b75ab278c22ce7c585e`. The root override compiles the
direct `CallKnown` closure of residual function 3, with bounded function,
bytecode, and emitted-code limits. The harness marks root-mode reports as
`qualification_ready: false`; neither report qualifies a performance win.

Both runs passed JetStream validation and output equality. The diagnostic run
shows that function 3 entered native code, with functions 22 and 23 also
entering native code. It compiled 22 of 35 functions, emitted 1,357,938 code
bytes, mapped 1,359,872 bytes, and spent 1.28 seconds compiling. The diagnostic
run is instrumentation-only and its Score/RSS values are not used below.

The uninstrumented screen measured:

| Runtime | Score | Peak RSS |
| --- | ---: | ---: |
| Quench JIT, root 3 | 54.45 | 305.3 MiB |
| Bun 1.4.3 | 207.01 | 50.6 MiB |
| Node 26.12.0 | 202.50 | 110.9 MiB |

Quench loses both Score and RSS against both JIT peers. Since the screen is far
from either threshold, no 11-round run was started. Keep this mode explicitly
opt-in; its Score is higher than the prior five-case Quench JIT HashSet result,
but this is an unpaired screen. Peak RSS is roughly 190 MiB above that result,
consistent with a large compilation-footprint cost.

Reproduction commands (from the repository root; production binaries were
built from the source commit above):

```sh
target/production/quench-bench --fixtures HashSet-wasm --suite jetstream3 --jit --jit-diagnostics --jit-wasm-root 3 --runs 1 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun --out target/hashset-wasm-root3-diagnostic-clean.json
target/production/quench-bench --fixtures HashSet-wasm --suite jetstream3 --jit --jit-wasm-root 3 --runs 1 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun --out target/hashset-wasm-root3-screen-clean.json
```
