# Zod `LoadEnvLocal → Call` fusion screen

The candidate fused an adjacent non-root lexical environment load and plain call in the general interpreter. It passed the targeted semantics test in interpreter and compiled modes, including closure reassignment, thrown calls, and a temporal-dead-zone read. The candidate was rejected after paired production screens because it regressed no-JIT speed and was effectively neutral in JIT mode.

The source revision includes the fetched `v2-merged` commit (`d99699911`) merged as `ad9432f5f`. Both production binaries came from that merged source. The candidate source was reverted after measurement.

| Mode / engine | Median ops/s | Median sampled process-tree RSS |
| --- | ---: | ---: |
| No-JIT candidate | 29,762 | 28.35 MiB |
| No-JIT merged-source control | 33,003 | 30.39 MiB |
| Node `--jitless` | 84,746 | 41.10 MiB |
| Bun no-JIT | 81,301 | 36.84 MiB |
| JIT candidate | 28,902 | 22.94 MiB |
| JIT merged-source control | 28,571 | 22.95 MiB |
| Node JIT | 285,714 | 50.39 MiB |
| Bun JIT | 344,828 | 48.92 MiB |

No-JIT candidate throughput was 9.8% below its paired control; RSS was 6.7% lower. JIT throughput was 1.2% above control, with effectively unchanged RSS. Both modes matched the expected checksum (`60000`). These are five-round diagnostic screens of 2,500 rounds and 100 warmups, not qualification measurements. Linux RSS is sampled every 50 ms as the process-tree sum of `/proc/<pid>/status` VmRSS.

Binary SHA-256:

- Candidate: `91f759fe9c17400f182295ec9e8d9200d59086a74fdbafda94007e70f040bfdb`
- Merged-source control: `37f4c0715cabca9867c15ac39bd80ce379cf9dfa59d4d9fdddef5882d610fb59`

Raw reports: `zod-fusion-no-jit-5.json` and `zod-fusion-jit-5.json`.

Next work should avoid runtime lookahead on every `LoadEnvLocal`; research points to compile-time fusion for profile-proven pairs or broader tagged-`Value` baseline execution with interpreter side exits. The latter is the larger JIT opportunity because current Zod hot helpers receive no native entries.
