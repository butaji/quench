> **High-level architecture of the Quench runtime.**

# Architecture Overview

Quench is a custom TypeScript/JavaScript/TSX runtime in Rust, split between a dedicated interpreter crate and the main terminal UI binary.

```
quench (main binary)
  ├── Bridge / event loop / terminal UI
  └── quench-runtime  (interpreter engine)
        ├── swc_parse  — parse JS/TS/TSX with swc
        ├── lower      — swc AST → runtime AST
        ├── lower_hir  — swc AST → HIR for explicit-stack execution
        ├── interpreter— execute AST (recursive legacy path)
        ├── shadow     — Self-Optimizing Shadow Tree Interpreter (explicit value/call stack)
        ├── builtins   — JS standard library
        ├── value      — JS values and objects
        └── context    — globals, environment, modules
```

## Source pipeline

1. `swc` parses `.js/.jsx/.ts/.tsx` source.
2. swc transforms strip TypeScript annotations and lower JSX to `ink.createElement`/`ink.Fragment`.
3. The lowerer produces either the runtime AST (legacy recursive interpreter) or a HIR (explicit-stack path).
4. Two execution paths are available:
   - **Legacy interpreter** — recursive AST walker with a depth counter.
   - **SSTI / HIR** — explicit value stack, explicit call frames, NaN-boxed values, and shape-based objects. This is the VM foundation for incremental conformance work.
5. Future performance work (full trampoline, JIT/AOT) stays deferred until after 100% conformance.

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

The legacy interpreter is recursive and uses `Rc<RefCell<...>>` for values. The SSTI/HIR path already uses NaN-boxed values, string interning, and object shapes. The planned evolution is:

1. **Conformance first** — drive test262 / TypeScript subsets to 100% pass using the existing explicit-stack paths.
2. **Complete trampoline migration** (Task 85) — once the legacy recursive path is no longer required for any subset, migrate all execution to explicit `Vec<CallFrame>`.
3. **Future performance work** only after 100% conformance — bytecode, JIT/AOT, and other optimizations stay deferred.

## Rust-specific leverage

Rust should be treated as an accelerator, not just a safe implementation language:

- **Ownership as VM invariant enforcement.** Pass `&mut Context` through evaluation and store objects in a `Vec`/`SlotMap` indexed by `ObjectId`. This removes `Rc<RefCell>` overhead from the hot loop and makes use-after-free impossible at compile time.
- **Zero-cost dispatch.** Keep `Value` as an enum and dispatch through `match`. Avoid `Box<dyn Trait>` / vtables in the interpreter.
- **Explicit JS stack.** Model the JS call stack as `Vec<CallFrame>` in a trampoline loop. This decouples JS recursion from the native Rust stack and enables `MAX_JS_STACK`, `try/catch`, generators, and `async/await` later.
- **Fearless concurrency for isolates.** Run each JS isolate in its own thread with no shared mutable state. Use channels for host communication. Parallelize the conformance runner with `rayon` and fresh contexts per test so one stack overflow does not kill the suite.
- **Allocation discipline.** Use `bumpalo` for short-lived AST/HIR allocations, `lasso` for identifier/property interning, and switch the global allocator to `mimalloc`/`jemallocator` once benchmarking starts.
- **Isolated `unsafe`.** Reserve `unsafe` for NaN-boxing and tightly encapsulated raw-pointer operations; verify with Miri. Do not use `unsafe` to bypass borrow-checker discomfort.

## Direction validation

The runtime now has two explicit-stack execution paths (SSTI and HIR) alongside the legacy recursive interpreter. Recent research and current test results confirm:

- **Explicit-stack execution prevents native stack overflow.** The SSTI path already runs deep recursive tests (e.g. `test_shadow_deep_chain_no_stack_overflow`) and complex examples without crashing. Task 85 (full trampoline migration) is an architectural improvement, not a blocker for conformance work.
- **Correctness before speed.** NaN-boxing, object shapes, and inline caches are already implemented in the SSTI path, but they are treated as enablers for conformance, not premature optimization.
- **test262 runners load harness files from the submodule** (`assert.js`, `sta.js`, etc.). The custom runner currently stubs some helpers; loading real harness files remains future work (Task 253 / 334).
- **Incremental conformance is the viable path.** Running whole suites at once produces an unactionable wall of failures. The stop-on-fail harness (`run_suite_stop_on_fail`) processes files in deterministic order and converts each failure into a focused regression test.

Implication: the VM foundation is ready; prioritize driving concrete test262 / TypeScript subsets to 100% pass using the incremental workflow.

## Pending work

See `tasks/index.json` for the current task list.
