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

1. **Trampoline interpreter** — explicit `Vec<CallFrame>` so JS recursion does not consume the native Rust stack.
2. **NaN-boxed `Value`**, string interning, and object shapes once correctness is solid.
3. **Future AOT/JIT** via Cranelift, consuming the same HIR.

## Pending work

See `tasks/index.json` for the current task list.
