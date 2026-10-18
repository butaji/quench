# GCC-Wasm root-first tier screen

This experiment starts with residual function 19 compiled at Cranelift
`opt_level=none`, then allows the existing bounded tier to promote hot fallback
functions. Strict root-only remains a separate mode and never promotes
fallbacks. Both modes are excluded from qualification reports.

The focused runtime tests passed for the four-function fallback chain: the
selected root returns the expected 1,680, its hot fallback function promotes
after 32 interpreted entries, and strict root-only leaves that function in the
VM. A separate invalid-root test confirms root-first does not promote another
function after admission fails. The diagnostic GCC run emitted function 19 at
`opt_level=none`; its measurements are instrumentation-only.

The uninstrumented 11-round JetStream 3 report is valid and output-equal, but
uses a dirty `v2-jit` worktree at source revision
`bb3d924ac50be782fe8ea2908b535802305148e4`. It is a focused one-fixture
experiment, not a clean-source lane qualification.

| Runtime | Median Score | Median peak RSS |
| --- | ---: | ---: |
| Quench root-first | 58.62 | 48.20 MiB |
| Bun 1.4.2 | 174.49 | 54.10 MiB |
| Node 26.10.0 | 179.90 | 62.38 MiB |

Paired median log-ratio bootstrap intervals were 0.351 (95% interval
0.327–0.359) for Quench/Bun Score and 0.329 (0.324–0.345) for Quench/Node
Score. Peak-RSS ratios were 0.893 (0.881–0.910) vs Bun and 0.773
(0.772–0.773) vs Node. Root-first keeps the RSS advantage but loses Score
decisively to both JIT peers. It is also below the prior selective-tier GCC
screen (71.28 median Score); that cross-window comparison is directional only.
The earlier strict root-only function-19 screen was 14.35 Score / 47.11 MiB,
also from a separate run, so it does not establish a paired root-first gain.

No JIT peer win resulted. Keep root-first as an explicitly opt-in diagnostic
policy only; the broader JIT goal remains open. The next experiment targets
the 307,270 of 309,039 imported callouts previously attributed to
`_emscripten_memcpy_js`.

Raw reports:

- `diagnostic.json` — one instrumented round; never used for performance.
- `screen-3-round.json` — preliminary uninstrumented screen.
- `gcc-11-round.json` — 11-round uninstrumented follow-up.

Reproduction commands from the repository root (production binaries built
with Rust 1.99.1):

```sh
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 --jit --jit-diagnostics --jit-wasm-root 19 --jit-wasm-root-first --runs 1 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun --out evidence/jit-jetstream3-wasm-2026-10-18/root-first/diagnostic.json
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 --jit --jit-wasm-root 19 --jit-wasm-root-first --runs 11 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun --out evidence/jit-jetstream3-wasm-2026-10-18/root-first/gcc-11-round.json
```

Production binary SHA-256: `quench-node`
`5515a5603bfa6896d1ad2b4012a61da8e3fc299b1b330010409953727a6fc0d4`,
`quench-bench`
`bf50a98c1c6a816228822c42fa9f8d26c8c9f565ece3b1fa5fcb0d3cbe284391`.
