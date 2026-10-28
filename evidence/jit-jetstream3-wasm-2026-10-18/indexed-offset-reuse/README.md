# QuickSort indexed-offset reuse experiments

Two default-off implementations of
`QUENCH_WASM_JIT_INDEXED_OFFSET_REUSE` were screened on JetStream 3
`quicksort-wasm`. Both maintained an exact i32-times-four companion and
replaced shift-by-two values while leaving wrapped address formation and
memory guards unchanged. Both failed the predeclared screen; the runtime
switch and its benchmark option were removed afterward.

The predeclared rule required the paired on/off Score ratio to reach 1.05 with
its 95% interval above a tie, while RSS growth stayed at or below 0.5 MiB.
The runner always marked these diagnostic reports non-qualifying. Every
reported round validated the JetStream output.

## Screen 1: all-register companions

The clean source was `816ea03afcb1d15850769aa92410351acadc1826`, with the
pinned corpus at `06785cf861ac44855f168cbbe829278c2802e6de`. Quench production
binary SHA-256 was
`c159313cf9cf0375dd184b75d271644819db4727abf7cb0c6dfa98e52b54e5d9`; the
benchmark binary SHA-256 was
`c261a6ca405b977f5cd526b5e765b3f63ff720248ec3457b696dfabf57c71f6a`.

| Engine | Median Score | Median peak RSS |
| --- | ---: | ---: |
| Quench reuse off | 194.34 | 43.00 MiB |
| Quench reuse on | 198.98 | 43.20 MiB |
| Bun 1.4.2 JIT | 330.18 | 47.11 MiB |
| Node 26.10.0 JIT | 411.13 | 61.63 MiB |

The paired on/off Score ratio is **1.0395** (95% interval **0.8684 to
1.1094**), below the predeclared 1.05 median threshold with the interval
crossing a tie. Peak RSS increased by 0.19 MiB. This failed the screen, so no
11-round confirmation ran. It also remained well below Bun and Node on Score.

Raw samples, analysis, and the resumable checkpoint are in `screen-3.json`,
`screen-3-analysis.json`, and `screen-3.checkpoint.json`.

## Screen 2: backward-use-slice companions

The second clean source was `e174c2f68ac76ae0962b7f18945ef6f85fe4c639`.
Quench production binary SHA-256 was
`ecbc031b4fb428d8133bf11f0a8559fb196e68b63defc641750d2e109da975e0`; the
benchmark binary remained
`c261a6ca405b977f5cd526b5e765b3f63ff720248ec3457b696dfabf57c71f6a`.

| Engine | Median Score | Median peak RSS |
| --- | ---: | ---: |
| Quench reuse off | 250.73 | 43.00 MiB |
| Quench reuse on | 208.89 | 42.82 MiB |
| Bun 1.4.2 JIT | 361.58 | 47.09 MiB |
| Node 26.10.0 JIT | 432.79 | 61.51 MiB |

The paired on/off Score ratio is **0.8320** (95% interval **0.8075 to
0.8383**), a clear regression. The paired RSS ratio is **1.0050**
(95% interval **0.9905 to 1.0138**), so RSS was effectively unchanged. The
candidate also lost Score to both peers. This failed variant was not advanced
to eleven rounds.

Raw samples, analysis, and the resumable checkpoint are in
`sparse-screen-3.json`, `sparse-screen-3-analysis.json`, and
`sparse-screen-3.checkpoint.json`.

Both versions failed to produce a repeatable local speedup. The optimization
is rejected; the earlier wrapped-address lowering remains the useful
QuickSort code-generation result.
