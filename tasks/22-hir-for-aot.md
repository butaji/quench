# Task 22: Stabilize the functional + reactive HIR for runtime and future AOT

## Goal

Define and stabilize a single **high-level intermediate representation (HIR)** inside `quench-runtime` that is:

1. **Functional** — expression-oriented, close to A-normal form (ANF), with explicit effects.
2. **Reactive** — first-class support for signals, memos, effects, and render boundaries.
3. **AOT-ready** — clean enough for the interpreter today and for a future AOT compiler later.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/ast.rs` (rename to `hir.rs` or keep `ast.rs` as the HIR module)
- `crates/quench-runtime/src/lower/`
- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/lib.rs`

## HIR requirements

### Functional core

- **ANF-like shape.** Most operations produce a single value bound to a name; sub-expressions are simple (literal, variable, parameter, global). Complex nested expressions are flattened during lowering.
- **Immutable values by default.** Mutations are explicit `Assign`, `SetProp`, `Delete`, or reactive `SignalSet` nodes.
- **Explicit effects.** I/O, bridge calls, rendering, and `Effect` nodes are marked so pure sub-expressions can be memoized, folded, or reordered safely.
- **First-class functions and closures.** `Function`/`ArrowFunction` nodes carry captured variables so closures work without re-parsing.

### Reactive primitives

The HIR should include nodes for the primitives that Quench/Ink actually uses:

- `Signal { id, initial }` — mutable reactive cell (maps to `useState`).
- `SignalGet { signal }` — read a signal inside a computation.
- `SignalSet { signal, value }` — write a signal.
- `Memo { id, deps, compute }` — cached derived value (maps to `useMemo`).
- `Effect { id, deps, callback }` — scheduled side effect (maps to `useEffect`).
- `Render { id, component, props }` — reactive component boundary; re-evaluates only when props or used signals change.

At runtime these nodes build a reactive dependency graph. At AOT time they let the backend generate efficient incremental update code.

### Language-agnostic surface

- After lowering, the HIR must contain no TypeScript-, JSX-, or swc-specific nodes. Type annotations, interfaces, namespaces, and JSX are gone.
- A future AOT backend should read the HIR without the original source or swc AST.
- Node set covers literals, identifiers, member access, calls, `new`, unary/binary operators, assignments, control flow, object/array literals with getters/setters/spread, template literals, and module import/export execution semantics.

### Serialization and diagnostics

- Derive `serde::Serialize`/`Deserialize` (behind a feature flag) so the HIR can be cached on disk and loaded by an AOT backend.
- Each node carries a `Span` (file, start/end line/col) for diagnostics; the AOT backend may discard spans.

## Steps

1. Audit `crates/quench-runtime/src/ast.rs`. Ensure every variant is AOT-lowering friendly.
2. Refactor nested expressions into ANF-style `Let` bindings during lowering.
3. Add `Signal`, `SignalGet`, `SignalSet`, `Memo`, `Effect`, and `Render` HIR nodes.
4. Detect hook calls (`useState`, `useMemo`, `useEffect`) and component calls during lowering and emit the corresponding reactive nodes.
5. Remove or lower any swc-specific leaks (swc atoms, raw swc node types).
6. Rename module/doc references from “runtime AST” to **HIR**.
7. Add `serde` derives behind a feature flag.
8. Add unit tests for `source -> swc -> HIR -> eval` and `source -> swc -> HIR -> serialize -> deserialize -> eval`.
9. Keep the HIR high-level enough for the interpreter but structured enough that a future AOT backend (e.g., Cranelift) can consume it directly. Do not add a bytecode layer now.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not change `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` and `tests/typescript/` are immutable.

## Pareto & reuse note

- Use `swc` only for parsing. The HIR must be plain Rust enums/structs.
- Do not build a full bytecode VM here; keep the HIR high-level.
- Defer SSA conversion, register allocation, and machine-code generation to the AOT backend.
- The reactive graph can be built lazily at runtime; do not require static analysis to be perfect before the interpreter can run.

## Acceptance criteria

- The interpreter executes the HIR, not the swc AST.
- A documented HIR node reference exists.
- A simple component using `useState` lowers to `Signal`/`SignalGet`/`SignalSet` nodes.
- A simple program round-trips through `source -> swc -> HIR -> eval`.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
