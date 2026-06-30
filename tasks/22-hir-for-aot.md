# Task 22: Stabilize the HIR for runtime and future AOT

## Goal

Define and stabilize a single **high-level intermediate representation (HIR)** inside `quench-runtime` that is used by the interpreter today and can be consumed by a future AOT compiler later.

## Files

- `crates/quench-runtime/src/ast.rs` (rename to `hir.rs` or keep `ast.rs` as the HIR module)
- `crates/quench-runtime/src/lower/`
- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/lib.rs`

## HIR requirements

1. **Language-agnostic.** After lowering, the HIR must contain no TypeScript-, JSX-, or swc-specific nodes. Type annotations, interfaces, namespaces, and JSX are gone.
2. **Self-contained.** A future AOT backend should be able to read the HIR without keeping the original source or the swc AST around.
3. **Stable node set.** The node enum should cover:
   - literals, identifiers, member access, calls, `new`, unary/binary operators, assignments
   - function declarations/expressions/arrow functions, parameters (with rest/destructuring support)
   - statements: block, var/let/const, if/while/do-while/for/for...of/for...in/switch/try-catch-finally/return/break/continue/throw/labeled
   - object/array literals with getters/setters/spread
   - template literals
   - module import/export execution semantics
4. **Serializable (optional but recommended).** Derive `serde::Serialize`/`Deserialize` so the HIR can be cached on disk and loaded by an AOT backend.
5. **Source-location preserving.** Each node should carry a `Span` or line/column for diagnostics, but the AOT backend does not need it.

## Steps

1. Audit `crates/quench-runtime/src/ast.rs`. Ensure every variant is something an AOT compiler could lower further (e.g., to bytecode or machine code) without re-parsing source.
2. Remove or lower any swc-specific leaks (e.g., swc atoms, swc spans as the primary key, raw swc node types).
3. Add `serde` derives behind a feature flag if binary size is a concern.
4. Rename the module/doc references from “runtime AST” to **HIR** so the architecture is explicit.
5. Add unit tests that round-trip a simple program through `source -> swc -> HIR -> eval`.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not change `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` and `tests/typescript/` are immutable.

## Pareto & reuse note

- Prefer existing `swc` only at the front door (parsing). The HIR itself should be plain Rust enums/structs with no swc dependency.
- Do not design a full bytecode VM in this task; keep the HIR high-level enough for the interpreter but structured enough for AOT.
- Defer low-level optimizations (constant folding, SSA, register allocation) to the AOT backend.

## Acceptance criteria

- The interpreter executes the HIR, not the swc AST.
- A documented HIR node reference exists (in `crates/quench-runtime/src/ast.rs` docs or a new `docs/hir.md`).
- A simple program can be parsed, lowered to HIR, and evaluated without touching the swc AST after lowering.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
