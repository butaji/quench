# Splay slab-local free-list screen — Linux, 2026-10-11

## Decision

Retain as an incremental Task 94 memory improvement; Splay remains unwon and
the Task 61 Score and RSS bars remain open. The heap now stores each freed
slot as a 16-bit offset within its 2,048-slot slab, plus a stack of nonempty
slabs. This replaces the global 32-bit index vector and keeps allocation
reuse grouped by slab.

Across two paired fixed-work batches, output matched in 22/22 rounds. Median
work RSS fell 890,880 B and setup-adjusted RSS fell 1,069,056 B. Raw
setup-adjusted wall time had a -4.70% median (12/22 wins; -44.61% to +85.67%),
but Linux hardware counters were unavailable (0/22 clean rounds). No speed
claim is made.

The separate stock sessions were output-equal. The baseline scored 2,332 vs
Node `--jitless` at 5,629, with 121,286,656 B max RSS vs Bun no-JIT at
62,275,584 B. The candidate scored 2,194 vs Node at 5,167, with 120,557,568 B
max RSS vs Bun at 61,886,464 B. Node and Bun drifted between sessions, so
these do not measure a causal Score change. Candidate RSS was 1.95x Bun; both
Task 61 bars remain unmet.

## Correctness

- Focused heap suite: 29/29 passed, including cross-slab reuse and weak handle
  generation overflow.
- Full runtime library suite: 549/550 passed. The one failure is the known
  unrelated German date-format expectation (`2.1.1970` vs `1.1.1970`).
- Test262 stage 47 (FinalizationRegistry): 47/47 passed.
- Test262 stages 79-81 (WeakMap, WeakRef, WeakSet): 255/255 passed.
- Production build completed with the repository's warnings-denied settings.
  No write barriers or JS/Wasm shared store paths changed.

## Build and measurements

- Candidate source starts at `fe169d1e1944805f6185f9f86bae3694208fd0af`.
- Rust: `1.99.0 (b940084d7 2026-09-28)`.
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs / CPU quota 4, 16 GiB
  memory limit; RSS via Linux `wait4`.
- Splay fixture SHA-256:
  `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`.
- Runner SHA-256:
  `924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb`.
- Baseline binary SHA-256:
  `0e48bf7c26545e16100853840df0f1989882b02ca077d3491fb19eac02a54ba0`.
- Candidate binary SHA-256:
  `2297b84522d927bebc1d03f70756342590014ed1d641982125a37ee187b6127e`.
- Candidate stock dependencies: Node 26.10.0 SHA-256
  `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`; Bun
  1.4.2 SHA-256 `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`;
  QuickJS SHA-256
  `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`.

Compressed raw reports are included for both fixed-work batches, baseline and
candidate stock sessions, and Test262 stages 47 and 79-81.

SHA-256:

- `candidate.patch`: `95a75b03c64c45f62e9f28536ae2e6821f743a87aaf2dd3c892b96a867181e62`
- `fixed-work-11.json.gz`: `f9af2f12df9d9d02cc4c5e9615ff7a2d7f98b973954d01a357ad5316eb412d99`
- `fixed-work-repeat-11.json.gz`: `d33bc836e9aadaf1c37c90f6b6428a9050ae8b015b373974174a2a22517632b2`
- `stock-baseline-11.json.gz`: `68d74dfbbd1c0c567e0c3c714f2c6cf423c6795ecfad0ecd432eac7f8ed18c7b`
- `stock-candidate-11.json.gz`: `43a196f6e019b4c1d43d9efac0b964b3b173e234bdfcea7e197973d3018267e1`
- `test262-stage-47.json.gz`: `ebc6ef29795108e358af5ed951c9651b49c3471415443c443d1b0757c8a544c1`
- `test262-stages-79-81.json.gz`: `4cc449175b4158ee1ba8a614b0d4d4f30e3be613c0a155d36079de4a24e1c2b8`

## Reproduction

Apply `candidate.patch` at the source commit above, build with Rust 1.99, then
run:

```sh
source /workspace/quench-build-env.sh
git apply evidence/task94-splay-slab-local-free-list-linux-2026-10-11/candidate.patch
cargo build --locked --offline --profile production -p quench-node --bin quench-node
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-v2-cloud-slotgen-early-bitcheck-baseline \
  --quench-peer work/task94-v2-cloud-slotgen-slab-free-candidate \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-v2-cloud-slotgen-slab-free-fixed-11.json
```

The second fixed-work report uses the same command with output path
`work/task94-v2-cloud-slotgen-slab-free-fixed-repeat-11.json`. Stock commands
are in the compressed reports.

Correctness commands:

```sh
CARGO_TARGET_DIR=target/task94-current CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked --offline -p quench-runtime --lib
CARGO_TARGET_DIR=target/iteration CARGO_INCREMENTAL=1 \
  cargo build --locked --offline --profile iteration -p quench-node --bin quench-node
TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=4 target/iteration/run-stages 47 47
TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=4 target/iteration/run-stages 79 81
```
