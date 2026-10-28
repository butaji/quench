# Guarded affine scale-4 address experiment

This candidate tried to lower wrapped Wasm addresses of the form
`base + (index << 2)` into a guarded 64-bit host address, retaining the exact
modulo-i32 expression on a slow branch. The focused regression covered a
carried address through the repeated-memory alias, shift-overflow fallback,
load/store equivalence, and fast/fallback out-of-bounds traps.

I rejected the candidate after the QuickSort comparison. The baseline and
candidate used the same `v2-jit` source revision (`40cb9a3bf539ee3a44e10d87eab9c6bfe14df5bf`), pinned JetStream 3.0 inputs, production profile, and 11 valid rounds each. Outputs matched for both reports. The measured Quench executable hashes were
`30242535f476c24e04f164974c6b1d7c0ad1a23bc452f4146f1bb0fa913bd8bd` for the
baseline and `30ba1baaa71c0b3751e1ebb6441028eeb2d95a2e0f7a700e853ee43e9a1324cd`
for the candidate.

| Build | Quench Score / peak RSS | Bun Score / peak RSS | Node Score / peak RSS |
| --- | ---: | ---: | ---: |
| Baseline | 221.73 / 42.80 MiB | 338.95 / 49.95 MiB | 431.95 / 88.19 MiB |
| Affine candidate | 149.23 / 43.81 MiB | 375.19 / 49.98 MiB | 431.22 / 88.27 MiB |

An independent 100,000-resample bootstrap of the two run windows estimates the
candidate/baseline Score ratio at `0.673` (95% interval `[0.611, 0.733]`) and
peak RSS ratio at `1.024` (`[1.017, 1.028]`). The Quench-to-Bun and
Quench-to-Node per-round Score ratios also worsened across windows. The
candidate kept Quench's absolute RSS below both peers, but reduced Score and
increased Quench RSS, so it made no progress toward the required joint win.

The windows ran sequentially rather than in randomized per-round order, and
the report covers QuickSort only. These are diagnostic rejection results,
not a qualification claim. The patch is archived as `candidate.patch.gz`
(decompress with `gzip -dc`); it is not part of the runtime source. The raw reports are
`quicksort-baseline-11-round.json` and `quicksort-affine-11-round.json`, with
their 100,000-resample comparison in `comparison.json`. An earlier three-round
candidate screen is in `quicksort-3-round.json`.

The focused regression passed before the final float join-type correction;
the corrected source passed `cargo +1.99.1 check --locked -p quench-runtime --tests`.
The production candidate and baseline builds both succeeded. I did not capture
native disassembly, so the expected scale-4 SIB formation and dead-code removal
were not verified directly.
