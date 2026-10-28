# Async-fs Promise lifecycle screen

The JIT-mode one-round screen was run after skipping weak-root tracking for
Promise resources when no `init` or `destroy` async hook is active. Promise
identity allocation and job context propagation remain enabled. The run passed
JetStream's output validation.

| Engine | Score | Maximum RSS |
| --- | ---: | ---: |
| Quench JIT | 3.44 | 891 MiB |
| Node JIT | 73.60 | 210 MiB |
| Bun JIT | 128.96 | 67 MiB |

The earlier one-round Quench production screen scored 1.31 and reported
7,791,362,048 bytes RSS (about 7.26 GiB). The candidate is a large memory
reduction, but remains far behind both peers on score and uses more RSS than
either. These single samples are screens, not qualification or proof of a
repeatable score gain.

Reproduce with:

```sh
cargo build --locked --profile production -p quench-node -p quench-bench
target/production/quench-bench async-fs --suite jetstream3 --jit --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node --node node --bun bun \
  --out evidence/jit-jetstream3-2026-10-21/async-fs-promise-lifecycle/screen.json
```

The raw candidate report is
`../async-fs-production-no-weakroots-matched-1round.json`; the earlier
production screen is `../async-fs-production-getter-1round.json`.
