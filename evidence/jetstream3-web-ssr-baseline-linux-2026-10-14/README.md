# JetStream 3 web-ssr baseline (Linux, diagnostic)

First matched run of `quench-bench --suite jetstream3 web-ssr --runs 1` after the
suite landed. One round, dirty source tree: a diagnostic baseline, not a
qualification run.

Host: Linux x86_64 cloud container, 4 vCPU, 15 GiB. Engines: Quench production
build, Node v26.10.0 `--jitless`, Bun 1.4.2 with `BUN_JSC_useJIT=0`.

| Engine | Score | Max RSS |
| -- | -- | -- |
| Quench | 3.06 | 800.6 MiB |
| Node --jitless | 58.52 | 185.8 MiB |
| Bun no-JIT | 54.81 | 118.3 MiB |

Callgrind of one marginal iteration (`eval` of the 755 KB bundle plus one
render) on the same build with symbols: 8.8 G instructions; collector 3.6 G
(41%, every collection is full while direct records exist, and every stored
eval program's constants are permanent roots), `eval` compile 1.4 G (16%),
`for-in` key enumeration 1.15 G (13%, a descriptor object per key).
