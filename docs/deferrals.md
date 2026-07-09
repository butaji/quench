> **Living registry of intentional conformance deferrals.** Every entry must point to an open task with an exact fix and exit criteria.

# Deferred Items

A test may be skipped only when the gap is tracked by an open task below. Do not use this file to record options or exploratory ideas.

## How to add a deferral

1. Create a task file in `tasks/` with the exact fix, target subset, and exit criteria.
2. Add the feature/gap to the table below with the task ID.
3. Update `tasks/index.json` by running `python3 scripts/target_tasks.py`.

## Current deferrals

| Feature / gap | Blocking? | Task | Exit criteria |
|---------------|-----------|------|---------------|
| ES module import/export | Yes | 241 | `tests/typescript` moduleResolution + `tests/test262/test/language/module-code/` subsets pass at 100%. |
| Promise / async / await / microtasks | Yes | 251 | `tests/test262/test/language/expressions/async-arrow-function/`, `async-function/`, `async-generator/`, and promise built-in subsets pass at 100%. |
| Generator functions and iterators | Yes | 251 | `tests/test262/test/language/statements/generators/`, `expressions/generators/`, and iterator protocol subsets pass at 100%. |
| Large file splitting (500-line limit) | Yes | 357 | All `*.rs` files in `crates/quench-runtime/src/` under 500 lines. |
| Proxy | No | — | Create a task before enabling any Proxy tests. |
| TypedArray / ArrayBuffer / DataView | No | — | Create a task before enabling any TypedArray tests. |
| WebAssembly host support | No | — | Out of scope until an example requires it. |

## Resolved decisions (no longer pending)

| Decision | Resolution | Task |
|----------|------------|------|
| Recursive vs iterative interpreter | Iterative trampoline with explicit `Vec<CallFrame>`; no recursive `eval_*`. | 85 |
| Parser choice | Stay on `swc_ecma_parser`; do not switch before 100% conformance. | 87 / docs/research-findings.md |
| Bytecode / JIT / AOT | Not used until 100% conformance; interpreter-only at this stage. | docs/minimum-custom-code-strategy.md |

## Skips policy

- Every skip in the test262/TypeScript harnesses must reference either a task in the table above or a specific open task file.
- "Not implemented yet" is not a justification without a task.
- A deferral is removed when its task closes and the corresponding subset passes at 100%.
