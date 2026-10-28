# zlib word-compare scan: rejected

This opt-in prototype replaces the eight sequential byte comparisons in the
pinned JetStream zlib `longest_match` function with two guarded little-endian
64-bit loads, XOR, and `ctz`. It is enabled only for the exact full-body
fingerprint of runtime function 11. It retains the original bytecode as the
scalar fallback when either eight-byte span reaches past the current Wasm
memory length. Compiled Wasm memories are unshared; the exact body has no
callouts or `memory.grow` during the scan. The equal path rejoins at bytecode
PC 70 and preserves the existing PC 73 backedge interrupt poll.

The one-round diagnostic confirms the full function fingerprint matched and
the native root received 7,704,769 entries. The optimized body was 1,847 bytes
(1,975 bytes in the earlier scalar diagnostic). The zlib fixture validated and
matched peer output in the diagnostic and every screen run.

## Results

| Mode | Rounds | Median Quench Score | Median Quench peak RSS |
| --- | ---: | ---: | ---: |
| Flag off | 3 | 13.34 | 112.81 MiB |
| Flag on | 3 | 13.46 | 112.86 MiB |
| Flag off | 11 | 13.46 | 112.797 MiB |
| Flag on | 11 | 13.47 | 112.859 MiB |

The 11-round reports ran separately, not as a randomized paired A/B. An
independent bootstrap of the two sets of 11 round medians (100,000 resamples,
seed 20261018) estimates a score difference of +0.01 points, with a 95%
interval of -0.21 to +0.23 points (about -1.56% to +1.71% of the flag-off
median). The median RSS difference is +0.0625 MiB, with a 95% interval of
+0.043 to +0.090 MiB. That is no score improvement signal and a small but
consistent RSS regression. The experiment fails its gate of a material score
gain without added RSS, so the specialization was reverted.

The candidate's 11-round medians were 13.47 Score / 112.859 MiB RSS for
Quench, 90.50 / 114.543 MiB for Bun 1.4.3, and 90.33 / 111.184 MiB for Node
v26.12.0. Quench remained far behind both peers on Score, below Bun on RSS,
and above Node on RSS. These are exploratory reports: the harness marked
`qualification_ready: false` because selective Wasm tiering is experimental.

## Reproduction

Both reports use source revision `ab37930e6c1fa8cb22b8458197ca9f9afdd4e6da`,
the pinned corpus revision `06785cf861ac44855f168cbbe829278c2802e6de`,
the same production Quench binary
`158bfa6182dcae4d7d827985864fe0a2e9c1bd66dc9bffd4b06029476dd5cfa0`, and
benchmark binary
`42d0399b049c1182dcebc29e0fc3fff7251451110dc6b0035da05c3ab39bd4c3`.
The only runtime toggle was `QUENCH_WASM_JIT_WORD_COMPARE_SCAN=1`, supplied by
a wrapper for the candidate report. Both modes used
`--jit --jit-wasm-selective`, three or eleven rounds, and the same Node/Bun
executables. The benchmark harness starts engines with an isolated
environment, so the wrapper is necessary to set the experimental variable.

`diagnostic-1round.json` captures the active fusion marker, compile statistics,
and validation. `flag-off-3round.json`, `flag-on-3round.json`,
`flag-off-11round.json`, and `flag-on-11round.json` contain raw samples.
`prototype.patch` preserves the rejected implementation for review; the
repository history records the experiment as `ab37930e6` and its revert as
`f0ae6d2b5`.

`cargo check --locked -p quench-runtime --lib --tests` passed. A targeted
`cargo test` build was attempted but rustc was killed by the workspace memory
limit while linking the full runtime test binary.
