# PGO training on a CPU-time budget (Linux x86-64, 2026-10-23)

Host: 4-vCPU Intel Xeon (Sapphire Rapids) cloud VM, 15 GiB RAM, no PMU counters.

Engines:
- Quench: `tools/pgo-build.sh release` at d61170f0da.
- References: `node --jitless` and `BUN_JSC_useJIT=0 bun`.

Fixtures: the V8-v7 suite as single-fixture Score runs.

Measurement: concurrent pairs, each run pinned to CPU 1 or CPU 2 (swapped every round), 7 rounds. A ratio is the median of Quench Score / reference Score.

## Finding

The training corpus ran each micro program for a wall-clock budget, so other load on the host changed each program's share of the profile. Two PGO builds of the same revision d61170f0da:

- **busy**: other jobs ran during training.
- **idle**: nothing else ran.

| Idle build Score / busy build Score | Ratio | Idle wins |
|---|---:|---:|
| NavierStokes | 1.197 | 7/7 |
| Richards | 1.077 | 5/7 |
| DeltaBlue | 1.059 | 5/7 |
| EarleyBoyer | 1.026 | 6/7 |

With the budget in the training process's own CPU time, a build trained while ten callgrind runs shared the 4 CPUs matches the idle wall-clock build:

| CPU-budget build (busy) Score / idle wall-budget build Score | Ratio |
|---|---:|
| NavierStokes | 0.989 |
| Richards | 1.012 |
| DeltaBlue | 0.968 |
| EarleyBoyer | 1.008 |
| RegExp | 0.973 |

Each of these ratios is within 3% of 1.

## Boards

| File | Build |
|---|---|
| `board-wall-busy.txt` | Wall-clock budget, busy host |
| `board-wall-idle.txt` | Wall-clock budget, idle host |
| `board-cpu-busy.txt` | CPU-time budget, busy host |

In `board-cpu-busy.txt`, every ratio vs Node --jitless and Bun JIT off is 1.22 or more except Richards vs Bun, at 1.19.
