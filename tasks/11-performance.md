> **See `docs/performance-research.md` for the full ranked analysis.**

# Task 11: Interpreter performance roadmap

## Goal

Apply interpreter-level optimizations so the runtime can run Ink apps at acceptable speed. AOT/JIT and bytecode compilation are out of scope for now.

> **Prerequisite:** Runtime passes the example suite and core conformance subsets. Remains `pending` until then.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Ranked optimization choices

From highest to lowest impact for a pure AST interpreter:

1. **NaN-boxed `Value` + inline small strings** — `Value` as a `Copy` 64-bit word.
2. **Object shapes (hidden classes)** — `shape_id → offset` instead of hashmap probing; inline 2–3 slots.
3. **Flattened lexical environments** — `Vec<Value>` slots from SWC scope analysis.
4. **Trampoline + operand stack** — explicit `Vec<Frame>` and `Vec<Value>` instead of recursive returns.
5. **String interning** — `lasso` atoms for identifiers and property names.
6. **Arena allocation** — `bumpalo` for frames and temporaries.
7. **Inline caches on HIR nodes** — cache `(ShapeId, offset)` for hot member/call sites.
8. **Fast maps & regex** — `indexmap`/`rustc-hash`, `regress`, `num-bigint`.
9. **Zero-cost host calls** — host functions take `&mut [Value]` slices.
10. **Global allocator** — `mimalloc` or `tikv-jemallocator`.

See `docs/performance-research.md` for the full rationale and current gaps.

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
   - Add `lasso` as a dependency.
   - Intern every identifier and property name at parse/lowering time.
   - Replace `HashMap<String, Value>` property storage with `IndexMap<Atom, Value>`.
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

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/counter.js
cargo run -- examples/animations.tsx
```
