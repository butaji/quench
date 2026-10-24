# V8-v7 max RSS board after the compiled-RegExp compaction

**Build:** v2-gc `1bc187d4b5` as a static production build, BOLTed with fresh profiles
(`pipe10.sh` / `pipe11.sh`: callgrind training on the deegen curriculum, micros and test262
order corpus, then `bolt-profile` and `llvm-bolt`).

**Board:** `quench-bench/v8-v7-rss.cjs`, 5 runs per engine, MemoryFirst. QuickJS, Node and Bun
are run as shipped.

**Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, 16 GB.

Ratio is Quench's max RSS over 0.8 × the best other engine's best run. The target is ≤ 1.00
on the worst run.

| Fixture | no-JIT worst (median) | --jit worst (median) |
| -- | --: | --: |
| Richards | 0.91 (0.86) | 0.95 (0.87) |
| DeltaBlue | 0.90 (0.90) | 0.94 (0.93) |
| Crypto | 1.03 (0.99) | 1.01 (0.98) |
| RayTrace | 1.04 (1.00) | 1.01 (1.00) |
| EarleyBoyer | 0.46 (0.45) | 0.46 (0.45) |
| RegExp | 1.00 (0.98) | 1.01 (0.99) |
| Splay | 0.64 (0.64) | 0.64 (0.64) |
| NavierStokes | 0.93 (0.93) | 0.94 (0.94) |

The previous board (`d7f732f66d`) had RegExp at 1.04 (1.03) and 1.08 (1.05). The other rows
are within run-to-run spread of it.

Remaining cuts on the worst run, in KiB:

| Row | no-JIT | --jit |
| -- | --: | --: |
| Crypto | 100 | 40 |
| RayTrace | 115 | 30 |
| RegExp | 0 | 70 |

Quench is below every other engine on every fixture.
