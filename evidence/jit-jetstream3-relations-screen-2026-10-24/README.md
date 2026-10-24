# JS GeneralDispatch relation screen

Source commit: `2583c662a8c1520ab0b5d1288ee4c1a58ef5ef90`

Host: Linux x86_64, 4 CPU quota, 16 GiB memory limit; Rust 1.99.0. JetStream 3 corpus: `06785cf861ac44855f168cbbe829278c2802e6de`. Engines: Quench production JIT, Bun 1.4.2 (`BUN_JSC_useJIT=1`), Node v24.19.0 (Maglev/TurboFan).

Both reports are 11/11 valid rounds and output-equal. Only `--jit-js-general-dispatch` differs:

| Mode | Quench score | Bun score | Node score | Quench RSS | Bun RSS | Node RSS |
|---|---:|---:|---:|---:|---:|---:|
| GeneralDispatch off | 12.33 | 70.88 | 69.28 | 113.6 MiB | 231.1 MiB | 178.9 MiB |
| GeneralDispatch on | 16.01 | 72.25 | 76.57 | 114.4 MiB | 233.6 MiB | 180.5 MiB |

The treatment is an experimental, non-qualification-ready mode. The sequential control/treatment comparison is diagnostic, not a causal win. Treatment's median per-round Quench score ratios are 0.217 vs Bun and 0.207 vs Node; its median RSS ratios are 0.49 and 0.63. This does not meet the 1.20 score target, while Quench RSS is lower on this fixture.

The raw reports record complete rounds, process metrics, engine hashes, host limits, input hashes, and output equality. The control analyzer artifact is included; the analyzer correctly refuses the experimental treatment as non-qualification-ready.
