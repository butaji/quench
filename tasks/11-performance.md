# Task 11: Interpreter performance roadmap

## Goal

Once the runtime is functionally correct, apply interpreter-level optimizations so it can run Ink apps at acceptable speed without requiring a JIT or bytecode VM.

> **Prerequisite:** Complete Tasks 01–08 first. This task is deliberately `pending` until the runtime passes the example suite.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/env.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/object.rs` (new, if extracted from `value.rs`)
- `crates/quench-runtime/src/scope.rs` (new)
- `Cargo.toml`

## Steps

1. **NaN-boxed / tagged `Value`**
   - Make `Value` a `Copy` 64-bit type.
   - Pack `f64`, object pointer, string pointer, and tags (`null`, `undefined`, `true`, `false`, int31) into a single `u64`.
   - Use `nanbox` or hand-roll the bit-packing.
   - Remove `Box<Value>`, `Rc<Value>`, and `RefCell<Value>` from hot paths; keep heap pointers only for objects, strings, and functions.

2. **String interning**
   - Add `lasso` (or `string-interner`) as a dependency.
   - Intern every identifier and property name at parse/lowering time.
   - Replace `HashMap<String, Value>` property storage with `HashMap<Atom, Value>` or `IndexMap<Atom, Value>`.
   - Keep non-interned string payloads for user string values using `compact_str` or `smol_str` if desired.

3. **Object shapes + inline caches**
   - Assign a `ShapeId` to every object. A shape stores property names as `Vec<Atom>` and a `HashMap<Atom, usize>` index.
   - Use a side-table keyed by AST `Span` or `NodeId` to cache `(expected_shape, offset)` for `MemberExpr`, `CallExpr`, and identifier lookups.
   - Fast path: `if obj.shape_id == cache.expected_shape { return obj.properties[cache.offset]; }`.

4. **Slot-indexed environments**
   - Run a scope-analysis pass over the SWC AST before execution.
   - Assign every local `let`/`const`/`var` a `u32` slot index.
   - Store function-call locals in a dense `Vec<Value>`; access via `locals[slot_idx]`.
   - Capture closures by reference into a flat `Environment` vector.

5. **Arena allocation**
   - Add `bumpalo`.
   - Arena-allocate call frames, temporary eval state, and short-lived objects.
   - Switch the global allocator to `mimalloc` or `tikv-jemallocator`.

6. **Explicit evaluation stack**
   - Replace recursive `eval_expression`/`eval_statement` with an explicit `Vec<Frame>` and a trampoline loop.
   - This also enables proper `try/catch/finally` and generators later.

7. **Faster maps and regex**
   - Use `rustc-hash`/`foldhash` for atom-keyed maps.
   - Add `regress` for ECMAScript regex and `num-bigint` for `BigInt`.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `Cargo.toml`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable; measure performance against existing examples.

## Acceptance criteria

- All functional tests still pass after each optimization phase.
- `cargo run -- examples/animations.tsx` runs without crashing and renders smoothly.
- A microbenchmark (e.g., 10,000 tight loop iterations) shows measurable improvement after each phase.

## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/counter.js
cargo run -- examples/animations.tsx
```
