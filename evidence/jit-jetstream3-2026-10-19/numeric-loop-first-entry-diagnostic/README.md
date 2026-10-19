# First-entry numeric-loop JIT diagnostic

This is a four-fixture, three-round Linux x86_64 development screen for the
numeric CFG compiler. JetStream outputs matched across Quench, Bun 1.4.3, and
Node 26.12.0. The report records a dirty working tree and is diagnostic only;
it is not a qualification run.

| Fixture | Quench Score / RSS | Bun Score / RSS | Node Score / RSS |
| --- | ---: | ---: | ---: |
| hash-map | 4.93 / 70.5 MiB | 282.92 / 87.6 MiB | 202.42 / 161.6 MiB |
| Stanford AES | 23.85 / 115.2 MiB | 305.13 / 56.5 MiB | 211.94 / 67.4 MiB |
| Stanford PBKDF2 | 33.86 / 67.0 MiB | 656.64 / 60.9 MiB | 386.59 / 62.4 MiB |
| Stanford SHA256 | 34.97 / 66.3 MiB | 684.87 / 56.3 MiB | 305.56 / 60.7 MiB |

The hash-map `equals` method executed natively 53,999,969 times, while the
benchmark's `runIteration` method remained unsupported. The crypto diagnostics
showed no native hot functions. First-entry promotion therefore did not close
the score gap, and Quench RSS also loses on AES. The next compiler work needs
to cover the hot object/string operations used by these workloads.

Reproduce with:

```sh
target/production/quench-bench \
  --fixtures hash-map,stanford-crypto-aes,stanford-crypto-pbkdf2,stanford-crypto-sha256 \
  --suite jetstream3 --jit --jit-diagnostics --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node work/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun work/bench-runtimes/bun-linux-x64/bun \
  --out work/js-hotloops-first-entry.json
```
