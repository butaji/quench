# Bound the closure-environment trace's cost (npm-lane regression report)

The npm lane bisected a 40% slowdown of typescript-eslint and @typescript-eslint/eslint-plugin
to `80594d2220`, which traces closure environments through what each closure reads. Its
Callgrind run found two costs:
- each environment kept a list of the closure edges it had followed, searched linearly;
- a scope that thousands of functions close over, as a module's is, walked a demand per
  function.

Change:
- **Dedup.** One trace-wide `FxHashSet` of (environment, program, function, distance) replaces
  the per-environment list. This is npm's patch (a).
- **Cost bound.** `PartialEnvironment::closure_cost` counts each closure edge and each slot its
  demand walks. Once the count passes `whole_cost(slots)` (each slot plus the parent edge),
  the environment is traced whole and later closure edges into it stop at once. The trace
  therefore costs at most about twice the whole-chain trace it refines. This replaces npm's
  patch (b), which counted the slots still unread.
- **Precision kept where edges are few.** All 30 closure-retention cases pass, the user's
  example among them. The new case `many_closures_over_one_scope` pins the bounded loss:
  seven kept closures of distinct functions over one scope keep its unread slot while they
  live.

Host: Linux 6.18 x86_64 VM, 4 CPUs. Static production builds, unbolted. Base is v2-gc
`12475d5020` (the tree that contains `80594d2220`), against base plus this change.

typescript-eslint under Callgrind, instructions:

| | Base | Change |
| -- | --: | --: |
| Program total | 7.86 G | 4.42 G (−44%) |
| `collect_slow` (inclusive) | 4.13 G | 0.69 G |
| `trace_environment_edges` | 3.55 G | 0.10 G |
| `read_slot` | 0.69 G | under the 1% cut |

npm's two patches together reached 5.76 G.

npm-top300 scenario CPU time (user + system), median of 7 alternating runs:

| Package | Mode | Base ms | Change ms |
| -- | -- | --: | --: |
| typescript-eslint | no JIT | 1545 | 1441 (−6.7%) |
| typescript-eslint | `--jit` | 1524 | 1313 (−13.8%) |
| @typescript-eslint/eslint-plugin | no JIT | 1531 | 1257 (−17.9%) |
| @typescript-eslint/eslint-plugin | `--jit` | 1445 | 1234 (−14.6%) |

V8-v7 MemoryFirst no-JIT max RSS, median of 7: every row moved by −116 to 0 KiB (RegExp −116,
Crypto −72). Scores are within noise.
