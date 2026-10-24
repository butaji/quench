# Focused JavaScript JIT oracle v1

This is a new, focused JIT parity inventory. It does **not** restore or claim the unavailable historical 116-script oracle. The 483 child-entry tests remain a separate Node host/API gate.

The oracle is Node.js v26.12.0 from the checked-in `tests/node` source revision `26e72d85a611e222e3f06025de8a3c496923bacd`. The manifest pins each fixture's SHA-256 and expected exit code, stdout, and stderr. Each case runs under Node, the Quench interpreter, and `quench-node --jit`; the JIT run enables general dispatch and stats, loops hot functions in each fixture, checks output parity, and fails unless the manifest's named functions report native entries.

From the repository root, run:

```sh
python3 crates/quench-runtime/tests/js-jit-oracles/v1/run.py
```

The runner builds `target/iteration/quench-node` by default with `cargo build --locked` and the runtime test profile settings (`CARGO_PROFILE_ITERATION_OPT_LEVEL=0`, `CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16`, `CARGO_BUILD_JOBS=2`). It prints the Node version, Quench source revision, and profile used. Use `--node PATH` or `--quench PATH` to select executables, or `--skip-build` to use an existing binary. `--timeout-secs` bounds each fixture; `--build-timeout-secs` bounds the build (900 seconds by default). A non-pinned local Node can be used for diagnostics with `--allow-node-version-mismatch`; that run is explicitly not pinned-oracle evidence.

The JIT oracle sets `QUENCH_JS_JIT_UNPRICED_GENERAL_DISPATCH=1` so hot candidates can be compiled for correctness even when the production payback model declines them. This switch is only for parity and forced-exit validation; never use these runs to compare speed, memory, or production tier profitability.

Each fixture performs 32,768 target calls before edge cases so the forced correctness mode reaches its hot-function promotion threshold. These calls do not establish production compile payback. The seven fixtures cover numeric speculation and fallback, field-read effects and changing receivers, call/throw effect order, locals/captures/TDZ, dense arrays and holes, and nested call reentry with live allocations. `guard_misses` records the semantic exits each fixture probes. Selected field fixtures also require per-function field-cache hits and more probes than hits, so a fallback path is exercised. Native-entry stats count function entries, not dynamic native instruction execution; neither those entries nor cache hits prove that GetField ran as native machine instructions. The current interface also lacks per-exit reason counters and a complete state-map verifier.
