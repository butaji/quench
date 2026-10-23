# JIT tier-manager transition smoke screen

Host: Linux x86_64, 4 vCPU quota, 16 GiB memory cgroup. The production binary
was rebuilt after moving JS and Wasm promotion state transitions under
`JitTierManager`; SHA-256:
`49848563687bf496cf38be61e71022f6e8e62f79f399ea9b4a28190100ddd4ed`.
Both stock `hash-map` runs validated output. Each mode is one round with the
stock JetStream harness and enabled diagnostics only where the mode requires
it; these are smoke screens, not performance evidence or qualification. The
source worktree was dirty while they ran because the source change was under
review.

| Mode | Quench Score / RSS | Bun Score / RSS | Node Score / RSS |
| -- | --: | --: | --: |
| no-JIT | 9.43 / 42.2 MiB | 9.83 / 47.7 MiB | 7.73 / 181.8 MiB |
| JIT | 10.15 / 37.6 MiB | 244.48 / 125.6 MiB | 234.86 / 193.2 MiB |

This refactor was intended to preserve the existing promotion transitions, not
improve generated code or score. The single JIT sample remains far behind both
JIT-enabled peers. Raw reports retain the executable IDs, command, host
metadata, per-engine outputs, and output-validation result.

Reproduction:

```sh
target/production/quench-bench --fixtures hash-map --suite jetstream3 --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/tier-manager-nojit.json

# Add --jit to the command and use a different --out path for JIT mode.
```
