# QuickSort forward vector scan screen: rejected

This experiment added a structurally matched four-lane i32 scan to the shared
Wasm JIT emitter. It keeps a scalar fallback when the full 16-byte range is
out of bounds or the address/index arithmetic could wrap. It also preserves
the original register writes and polls the interrupt flag after each true
lane. The instrumented run confirmed the matcher on residual function 1 at
bytecode `pc=9`; output validation passed.

The uninstrumented three-round iteration-profile screen regressed Quench's
median QuickSort score by 13.3%, while lowering peak RSS by 1.7% (about
0.78 MiB). Quench's RSS was already below Bun and Node, while Score remained
well below both peers. This did not advance the required joint win, so the
code change was removed without an 11-round run.

| Build | Quench Score / peak RSS | Bun 1.4.3 | Node v26.12.0 |
| --- | ---: | ---: | ---: |
| Baseline | 109.25 / 45.58 MiB | 384.64 / 47.13 MiB | 476.23 / 88.43 MiB |
| Vector scan | 94.68 / 44.80 MiB | 367.16 / 47.08 MiB | 506.75 / 88.40 MiB |

Both reports completed all three rounds with valid, equal output. The score
ratio is a comparison of separate three-round windows, not a paired estimate;
the reports are diagnostic screens and `qualification_ready` is false. They
use the pinned JetStream 3.0 checkout at `06785cf861ac44855f168cbbe829278c2802e6de`.
The source commit was `60e5f4df6e1f0d5985b5faea520514609d1c787a` with the
experimental compiler file dirty. The baseline and candidate Quench binary
SHA-256 values are `ffeddbf045c337a4d6cd0fdaa0ecabdec3226a4d1eeac310b1d895fe4bee9c60`
and `8f5d967a6f6d855d36b03105bf1e2a3cd36ff1fdcfff87beebaed8c3737ef581`.

The diagnostic report includes compile-counter overhead and is only used to
show that the specialization matched. The other two reports are the
uninstrumented score/RSS screen. `candidate.patch.gz` archives the rejected
implementation for future reference.

Reproduce the screen after building the iteration profile:

```sh
target/iteration/quench-bench --fixtures quicksort-wasm --suite jetstream3 --jit --runs 3 --timeout-ms 600000 \
  --quench target/iteration/quench-node-before-forward-scan \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/quicksort-forward-scan-baseline-3round.json

target/iteration/quench-bench --fixtures quicksort-wasm --suite jetstream3 --jit --runs 3 --timeout-ms 600000 \
  --quench target/iteration/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/quicksort-forward-scan-candidate-3round.json
```
