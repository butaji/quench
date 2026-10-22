# No-JIT Acorn and Babylon screen

Three randomized paired rounds per fixture and engine using the stock JetStream
3 driver. This is a local optimization screen, not a qualification run. Source
revision `ffa6655230838cdcedd8a64a0de6ab80b9315ef8` was clean. Linux x86_64,
Host reports `cpu_quota=400000/100000` (4 CPUs) and a 16 GiB
cgroup memory limit. Runtime and harness hashes, per-round samples, execution
order and output-equality records are in `report.json`.

| Fixture | Quench score | Bun no-JIT | Node `--jitless` | Quench/Bun | Quench peak RSS |
| -- | --: | --: | --: | --: | --: |
| Acorn WTB | 1.80 | 2.01 | 3.12 | 0.896x | 49.9 MiB |
| Babylon WTB | 2.24 | 3.24 | 4.71 | 0.691x | 46.4 MiB |

Both fixtures validated and Quench output matched the reference engines. Peak
RSS was below Bun and Node in both screens. The short run does not support a
qualification claim or a 20% score advantage.

## Decision on the `DefineField` lane-exit lead

The earlier instrumented lane-exit census recorded 1,096,767 `DefineField`
exits in Babylon out of 1,455,297,689 dispatched lane instructions (about
0.075%). Acorn had 4,743 such exits out of 2,651,935,416 instructions. This
small executed share does not justify a new handler on its own; exits are not
time attribution, and the added path would have to preserve the exact throw and
side-effect continuation for frameless activations. Do not implement it until
profiling shows material exclusive time. The census is archived at
`evidence/nojit-lane-exits-acorn-linux-2026-10-22/`.

## Reproduction

```sh
target/production/quench-bench --fixtures acorn-wtb,babylon-wtb \
  --suite jetstream3 --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/nojit-definefield-baseline.json
```

Only Quench, Bun and Node were measured here; the harness omitted QuickJS.
