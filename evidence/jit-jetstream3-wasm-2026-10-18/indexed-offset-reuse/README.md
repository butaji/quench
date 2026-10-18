# QuickSort indexed-offset reuse A/B

This experiment screens the default-off `QUENCH_WASM_JIT_INDEXED_OFFSET_REUSE`
candidate on JetStream 3 `quicksort-wasm`. The candidate maintains an exact
i32-times-four companion and replaces shift-by-two values in structurally
matching functions. Existing wrapped address formation and memory guards are
unchanged.

The first screen is predeclared at three paired rounds. Continue to eleven
rounds only if the on/off paired Score ratio is at least 1.05 with its 95%
interval above a tie, and peak RSS increases by at most 0.5 MiB. The runner
always marks this diagnostic as non-qualifying. A confirmed local improvement
still does not count as a peer win unless the same report also puts Quench
above both Bun and Node on Score and below both on peak RSS.

The focused regression type-checks. A public-API smoke run matched the
interpreter, compiled-off, and compiled-on paths on ordinary indices,
modulo-wrapped indices, and out-of-bounds traps. The full unit-test harness
could not finish compiling within the container's memory limit; no assertions
from that harness ran.

Run commands, source/binary hashes, raw samples, paired analysis, and the
screen outcome are stored in `screen-3.json`,
`screen-3-analysis.json`, and `screen-3.checkpoint.json`.

## Screen 1: all-register companions

The clean source was `816ea03afcb1d15850769aa92410351acadc1826`, with the
pinned corpus at `06785cf861ac44855f168cbbe829278c2802e6de`. Quench production
binary SHA-256 was
`c159313cf9cf0375dd184b75d271644819db4727abf7cb0c6dfa98e52b54e5d9`; the
benchmark binary SHA-256 was
`c261a6ca405b977f5cd526b5e765b3f63ff720248ec3457b696dfabf57c71f6a`. The
three rounds rotated Quench arms and peers; every output validated.

| Engine | Median Score | Median peak RSS |
| --- | ---: | ---: |
| Quench reuse off | 194.34 | 43.00 MiB |
| Quench reuse on | 198.98 | 43.20 MiB |
| Bun 1.4.2 JIT | 330.18 | 47.11 MiB |
| Node 26.10.0 JIT | 411.13 | 61.63 MiB |

The paired on/off Score ratio is **1.0395** (95% interval **0.8684 to
1.1094**), below the predeclared 1.05 median threshold and with the interval
crossing a tie. Peak RSS increased by 0.19 MiB. The candidate remains well
below Bun and Node on Score, though below their RSS. It fails the screen, so
the 11-round confirmation was not run. A narrower prototype will track only
registers that feed shift-by-two operations; that is a new candidate and will
need its own predeclared screen.
