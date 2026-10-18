# Inline storage at the Wasm import boundary

Commit `280b9a011` replaces the cloned Wasm parameter/result vectors and
converted JavaScript argument vector in the generic import path with inline
`SmallVec` storage (heap spill above eight entries). Per-instance target
resolution, JS conversions, dynamic helper lookup, and the generic call path
remain intact.

Both reports use the same production benchmark harness at clean source
revision `280b9a011`, Node v26.12.0, Bun 1.4.3, two JetStream 3.0 fixtures,
11 rounds, valid outputs, and equal results. The baseline report ran the saved
Quench executable from `e955cff99` (SHA-256
`6cdd85fefcdba3983eab5f3cd1aa371bbb6668404e7ffa3122670f72783c28cd`); the
optimized report ran the `280b9a011` executable (SHA-256
`cf909de333e83d970fce804bb0815baca25c482b4d94f6d5c79940b21b5fc0ef`). This
is a consecutive same-host A/B screen, not a randomized paired comparison
between source revisions.

| Fixture | Version | Quench Score / RSS MiB | Bun Score / RSS MiB | Node Score / RSS MiB |
| --- | --- | ---: | ---: | ---: |
| gcc-loops Wasm | baseline | 58.68 / 48.64 | 190.92 / 51.12 | 179.78 / 77.39 |
|  | SmallVec | 57.91 / 47.81 | 174.76 / 51.05 | 183.66 / 77.34 |
| QuickSort Wasm | baseline | 176.28 / 43.50 | 357.99 / 43.99 | 428.36 / 74.04 |
|  | SmallVec | 170.62 / 42.44 | 332.74 / 44.04 | 422.53 / 74.06 |

The optimized report lowers Quench's median peak RSS by 0.83 MiB on gcc-loops
and 1.06 MiB on QuickSort, while its Score medians move down 1.3% and 3.2%.
Bun's medians also fell between runs, so this screen does not establish a
causal Score regression or gain. In the optimized report, Quench's paired RSS
ratios to Bun are 0.936 [0.927, 0.951] on gcc-loops and 0.967 [0.962, 0.970]
on QuickSort; Score ratios are 0.337 [0.323, 0.348] and 0.437 [0.405, 0.536].
Both still lose Score to Bun and Node. The allocation reduction is semantically
verified by the 713-test runtime suite and may reduce allocator pressure, but
it has not yet demonstrated a Score win. More boundary overhead remains: the
307,270 `copyWithin` imports still pass through normal VM/JS dispatch.

The raw reports preserve executable hashes, engine versions, round samples,
input hashes, validation, and output equality. Analyzer `qualification_ready`
is true for each harness run; performance qualification fails on Score.
