# Wasm leaf inlining streamed from the frontend's sources (2026-10-18)

Linux x86_64 cloud VM, 4 vCPU, 16 GB. Static production `quench-node` (qualification flags in
`docs/README.md`), base `v2-merged` 30b82ee9b, candidate the same plus this change.

## Change

`lower_wasm_module_for` used to decode every body into a `Vec<wasmparser::Operator>` (56 bytes an
operator) before lowering, copy each inlining round's leaf bodies, and hold one inlined body
while lowering it. Bodies now stay in the frontend's re-readable `OperatorsReader`; a body's
form after each inlining round is a stream (the previous form with that round's leaf calls
replaced by the leaves' previous forms), and what lowering and the next round need of a form
(locals, first writes, directly accessed memories, globals, length, calls) follows from the
previous form and the copies one pass over it makes. Lowering then reads each body once per
round that can still copy into it, plus once to emit it.

Equivalence: a temporary cross-check (removed before commit) compared, for every lowered body,
the streamed operators and the derived locals, first writes, memories and globals against the
old materializing inliner. Wasm testsuite: 67,124/67,124 directives no-JIT and the JIT-mode run
identical apart from `call_indirect.wast`, whose JIT worker overflows its stack on the base too.
sqlite3 `speedtest1.wasm`: 2,533/2,533 bodies identical.

## Results (max RSS, KiB)

| Scenario | Base | Candidate |
| -- | --: | --: |
| CoreMark harness (`run-js.mjs`) through instantiation, no JIT | 16,464 | 15,804–15,928 |
| CoreMark module compile over an array-literal baseline | +990 | +650 |
| `new WebAssembly.Module(speedtest1.wasm)` twice, no JIT | 90,568 | 33,012–33,348 |
| the same, `--jit` | 52,988 | 26,984 |

Compile cost (Callgrind, one no-JIT `new WebAssembly.Module(speedtest1.wasm)` over the
`readFileSync` baseline): 1,064M → 1,345M instructions (+26%): every pass decodes again instead
of walking a vector. JIT compile wall time unchanged (85–90 ms).

All-eight V8-v7 guard (`callgrind-guard.txt`): per-run deltas within ±0.00%.

Checks: `cargo test --release -p quench-wasm` 35/35; runtime lib 709/709; Node `framework-core`
43/43; Node `--filter wasm` 0/3 on both base and candidate (pre-existing).
