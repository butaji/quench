> **High-level architecture of the Quench runtime.**

# Architecture Overview

Quench is a custom TypeScript/JavaScript/TSX runtime in Rust, split between a dedicated interpreter crate and the main terminal UI binary.

```
quench (main binary)
  ├── Bridge / event loop / terminal UI
  └── quench-runtime  (interpreter engine)
        ├── swc_parse  — parse JS/TS/TSX with swc
        ├── lower      — swc AST → HIR
        ├── interpreter— execute HIR
        ├── builtins   — JS standard library
        ├── value      — JS values and objects
        └── context    — globals, environment, modules
```

## Source pipeline

1. `swc` parses `.js/.jsx/.ts/.tsx` source.
2. swc transforms strip TypeScript annotations and lower JSX to `ink.createElement`/`ink.Fragment`.
3. The lowerer produces a high-level IR (HIR).
4. The interpreter executes the HIR.

## Key design choices

- **No cross-compilation.** The runtime executes source directly; no `esbuild`/`tsc` step.
- **Dedicated crate.** `crates/quench-runtime/` is isolated from the main binary.
- **Native host-function API.** `src/main.rs` registers `__ink_*` functions without coupling crates.
- **Shared prototypes.** `Object`, `Array`, `Function`, `String`, `Date`, `Map`, `Set` share prototype objects.

## Boundaries

Do not modify:

- `src/bridge/`, `src/ink/`, `src/render/`
- `examples/`, `tests/test262/`, `tests/typescript/`

Allowed glue points:

- `crates/quench-runtime/src/`
- `src/main.rs` for host-function registration
- `src/event_loop.rs` for JS dispatch calls
- `src/runtime.js` for targeted compatibility shims

## Testing

- Unit/regression tests: `crates/quench-runtime/tests/`
- Example integration: `cargo run -- examples/counter.js`
- Conformance: see `docs/conformance.md`

## Performance direction

The interpreter is currently recursive and uses `Rc<RefCell<...>>` for values. The planned evolution is:

1. **Trampoline interpreter** — explicit `Vec<CallFrame>` so JS recursion does not consume the native Rust stack. Use `&mut Context` and slot-indexed storage to keep the hot loop borrow-checker friendly.
2. **NaN-boxed `Value`**, string interning, and object shapes once correctness is solid.
3. **Future performance work** only after 100% conformance — object shapes, inline caches, and other optimizations that stay within the interpreter/HIR model.

## Rust-specific leverage

Rust should be treated as an accelerator, not just a safe implementation language:

- **Ownership as VM invariant enforcement.** Pass `&mut Context` through evaluation and store objects in a `Vec`/`SlotMap` indexed by `ObjectId`. This removes `Rc<RefCell>` overhead from the hot loop and makes use-after-free impossible at compile time.
- **Zero-cost dispatch.** Keep `Value` as an enum and dispatch through `match`. Avoid `Box<dyn Trait>` / vtables in the interpreter.
- **Explicit JS stack.** Model the JS call stack as `Vec<CallFrame>` in a trampoline loop. This decouples JS recursion from the native Rust stack and enables `MAX_JS_STACK`, `try/catch`, generators, and `async/await` later.
- **Fearless concurrency for isolates.** Run each JS isolate in its own thread with no shared mutable state. Use channels for host communication. Parallelize the conformance runner with `rayon` and fresh contexts per test so one stack overflow does not kill the suite.
- **Allocation discipline.** Use `bumpalo` for short-lived AST/HIR allocations, `lasso` for identifier/property interning, and switch the global allocator to `mimalloc`/`jemallocator` once benchmarking starts.
- **Isolated `unsafe`.** Reserve `unsafe` for NaN-boxing and tightly encapsulated raw-pointer operations; verify with Miri. Do not use `unsafe` to bypass borrow-checker discomfort.

## Direction validation

Recent research confirms the current shape is the right one for an Ink-focused runtime:

- **Recursive AST interpreter is the simplest path to correctness**, but every production engine uses an explicit call stack / trampoline to avoid native stack overflow. The planned trampoline interpreter (Task 85) is the canonical fix.
- **Correctness before speed.** Stay with the AST / HIR interpreter until the conformance suites pass; only then invest in object shapes, NaN-boxing, or other performance work.
- **test262 runners load harness files from the submodule** (`assert.js`, `sta.js`, etc.) and run them in the engine before each test. We should do the same instead of stubbing helpers.
- **Value representation can stay simple for now.** Boa and QuickJS use dedicated `JsValue` enums and reference counting early on; NaN-boxing and shapes are optimizations, not correctness prerequisites.
- **String interning, object shapes, and ICs are the standard performance stack** once correctness is achieved.

Implication: keep prioritizing low-effort conformance wins and the trampoline interpreter over premature optimization.

## Pending work

See `tasks/index.json` for the current task list.
