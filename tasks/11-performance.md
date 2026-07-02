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

## Implementation recipe

The target is a QuickJS-competitive pure AST interpreter, not a bytecode VM.

1. **`Value` becomes `Copy` `u64`.**
   - Quiet-NaN packing with 4-bit tag and 48-bit payload.
   - Inline integers and small strings; heap pointers for objects/functions/large strings.

2. **Strings become `Atom(u32)`.**
   - Use `swc_atoms` or `lasso` at parse time.
   - Property names and identifiers are never compared as byte strings at runtime.

3. **Execution becomes an explicit stack machine over the AST.**
   - `Vm { stack: Vec<u64>, frames: Vec<Frame> }`.
   - `eval_expr` pushes/pops the operand stack; no recursive `Value` returns.
   - Function call: push args, push `Frame { fp, env, return_expr }`, eval body, pop frame, push result.
   - Use a trampoline loop so the Rust call stack stays at O(1) regardless of JS recursion depth.
   - Add a configurable `MAX_JS_STACK` guard (e.g., 10,000) to throw JS `RangeError` instead of native stack overflow.
   - Mark tail calls in the AST so tail-recursive JS functions reuse the current frame.

4. **Scopes become flat `Vec<u64>` arrays.**
   - SWC scope analysis gives each local a `(scope, index)`.
   - Access via `env.values[index]`; no `HashMap`, no `RefCell` in hot paths.

5. **Objects get shapes + inline slots.**
   - `Object { shape: &Shape, proto: Option<&Object>, slots: [JsValue; 4], overflow: Option<&mut [JsValue]> }`.
   - Shape transitions share structure; property lookup is `shape.index[atom]`.
   - Dense arrays use a contiguous `Vec<Value>` for `0..length`.

6. **Inline caches on HIR nodes.**
   - `MemberExpr` carries `ic: Cell<Option<(&Shape, u32)>>`.
   - Monomorphic fast path; slow path resolves and caches.

7. **Specialized fast paths and host-call bridge.**
   - Inline `int+int`, `string+string`, etc. before generic coercion.
   - Host functions take `&mut [u64]` slices into the operand stack.

8. **Arena allocation.**
   - `bumpalo` for AST nodes, call frames, temporary values, and overflow slots.
   - Optional global allocator: `mimalloc` / `tikv-jemallocator`.

9. **Faster maps and built-ins.**
   - `rustc-hash` for shape transitions; `indexmap` where order matters.
   - `regress` for regex, `num-bigint` for `BigInt`.

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
