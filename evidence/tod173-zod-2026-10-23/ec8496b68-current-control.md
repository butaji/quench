# Zod control after `v2-merged` ec8496b68

This five-round diagnostic is the clean control for the current merged source, commit
`9a80b9282` (which includes upstream `ec8496b68`). It used the production `quench-node`
binary with SHA-256
`bfef0e0995d089f97cafbe16cbb1a50f607f4cd66bff4bc2117f836c2846fda4`, 2,500 benchmark
rounds, 100 warmups, and pinned `zod@4.6.7`. All engines matched checksum `60000`.

## No-JIT

| Engine | Median operations/s | Median sampled process-tree RSS |
| --- | ---: | ---: |
| Quench interpreter | 26,882 | 28.58 MiB |
| Node `--jitless` | 82,645 | 42.12 MiB |
| Bun no-JIT | 59,172 | 36.81 MiB |

## JIT

| Engine | Median operations/s | Median sampled process-tree RSS |
| --- | ---: | ---: |
| Quench `--jit` | 23,474 | 23.14 MiB |
| Node | 263,158 | 48.68 MiB |
| Bun | 344,828 | 45.40 MiB |

These samples show a clear RSS advantage but a large throughput deficit. They are
diagnostic screens, not qualification results; Linux RSS is sampled every 50 ms as the
process-tree sum of `/proc/<pid>/status` VmRSS. Raw reports:
`ec8496b68-no-jit-control-5.json` and `ec8496b68-jit-control-5.json`.
