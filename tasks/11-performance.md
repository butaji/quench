# Task 11: Interpreter performance roadmap

## Goal

Once the runtime is functionally correct, apply interpreter-level optimizations so it can run Ink apps at acceptable speed. AOT/JIT and bytecode compilation are explicitly out of scope for now.

> **Prerequisite:** Complete Tasks 01–08 first. This task is deliberately `pending` until the runtime passes the example suite.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Research-backed recommendations

- **Do not write a parser or lexer from scratch.** `swc` is the right front-end; it is already in use.
- **Prefer interpreter-level wins first.** AOT/JIT and bytecode are future work; the immediate gains are NaN-boxed values, string interning, object shapes + inline caches, slot-indexed environments, arena allocation, and an explicit eval stack.
- **Adopt hidden-class-style object shapes + inline caches (ICs).** This is the single most important JS-specific optimization (used by V8, SpiderMonkey, Hermes, Boa, etc.). Cache `(expected_shape, offset)` on hot property/member/call sites.
- **NaN-box primitives.** Pack `f64`, object pointer, string pointer, and small tags into a single `u64` so `Value` is `Copy` and 64-bit.
- **Intern identifiers and property names.** Use `lasso` or `string-interner` so property maps have integer keys and fast hashing.
- **Use `indexmap` for object properties** when deterministic enumeration order is required; otherwise use `rustc-hash`/`foldhash` with atom keys.
- **Arena-allocate short-lived state** with `bumpalo`; consider `mimalloc`/`tikv-jemallocator` as the global allocator.
- **Use existing crates for diagnostics** (`thiserror`, `miette`/`ariadne`) instead of hand-rolling a formatter.

## Files

- `crates/quench-runtime/src/value/` (value representation, shapes)
- `crates/quench-runtime/src/interpreter/` (eval loop, ICs)
- `crates/quench-runtime/src/env.rs` (slot-indexed scopes)
- `crates/quench-runtime/src/lower/` (HIR)
- `crates/quench-runtime/src/ast.rs` (HIR nodes)
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
   - Use a side-table keyed by HIR node id to cache `(expected_shape, offset)` for `MemberExpr`, `CallExpr`, and identifier lookups.
   - Fast path: `if obj.shape_id == cache.expected_shape { return obj.properties[cache.offset]; }`.

4. **Slot-indexed environments**
   - Run a scope-analysis pass over the HIR before execution.
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

## Future direction (not in this task)

- AOT/JIT via Cranelift (`cranelift-module`, `cranelift-object`, `cranelift-jit`) can be added later, consuming the same HIR directly.
- Bytecode compilation is also future work; the HIR should stay high-level enough to avoid needing it as an intermediate step.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `Cargo.toml`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable; measure performance against existing examples.
- Do not add a bytecode VM or Cranelift backend in this task.

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
