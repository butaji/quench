# Goal: Finish the custom TS/JS/TSX runtime and make it fully Ink-compatible

`quench-runtime` already exists as a dedicated workspace crate under `crates/quench-runtime/`, and `rquickjs` has been removed from the main crate. The remaining work is to close the interpreter gaps so that `runtime.js`, compiled TSX/JSX output, and all Ink examples run correctly without touching the bridge, renderer, compiler, or native Ink runtime.

> **One-line rule:** We write the **execution engine** (parser lowering, value model, eval loop, built-ins, host-function glue). We do **not** write a parser — **swc** parses JS/TS/TSX. We use crates for parsing; standard-library objects are implemented in Rust because they are the engine's observable JS environment.

## Current state

- `crates/quench-runtime/` has a working skeleton: swc parser, runtime AST, interpreter, value/object model, and built-ins.
- Recent progress:
  - Parser/lowering: computed member access, template literal expressions, optional chaining member and call (`?.prop`, `?.[expr]`, `?.()`), `for...of`/`for...in`, rest parameters, nullish coalescing (`??`), `in`, `instanceof`.
  - Built-ins: most `Array.prototype` methods, `Map`/`Set` constructors and methods, `String.prototype` methods (`repeat`, `padStart`, etc.), `Date.prototype` getters, `Object.prototype` (`hasOwnProperty`, etc.), real `JSON.parse`, native `Error` constructors.
  - Interpreter: recursion-depth guard, `arguments` object, rest-param binding, `Function.prototype.call/apply`, `New` expression wiring, bound methods.
  - Bridge/event loop: `__ink_get_node_parent`, `__ink_set_raw_mode`, `__ink_stdin_is_raw`, populated `__ink_get_node_children`, JSON props serialization, microtask draining, hot-reload re-registration.
  - Compiler: hooks are no longer incorrectly prefixed inside `createElement` calls.
- Still missing (now the main blockers for real examples):
  - Parser: module/script fallback, real getter/setter metadata, object/array spread.
  - Interpreter: spread expansion, getter/setter invocation, `typeof` on undeclared identifiers.
  - Built-ins: `Promise`, iterable `Map`/`Set` for `for...of` and `Array.from`, `Date.prototype.toLocaleTimeString`, shared `Function.prototype`, boxing constructors.
  - Code health: `builtins.rs`, `interpreter.rs`, `lower.rs`, and `value.rs` exceed the 500-line file limit; `build.rs` linter does not yet cover the runtime crate.
- `runtime.js` now parses most of its constructs instead of silently dropping them.
- `examples/simple.js` is expected to work; `counter.js`, `use-bridge.tsx`, and `animations.tsx` are now blocked mainly by spread, iterable Map/Set, Promise, getters/setters, and `typeof` undeclared.

## Approach

1. **Fix parser/lowering bugs** — support module/script parsing, fix computed member access, template literal expressions, optional chaining, rest parameters, `for...of`/`for...in`, nullish coalescing, getters/setters.
2. **Complete standard-library objects** — real mutating `Array.prototype`, `Map`/`Set` methods and iterators, `Promise`, `String.prototype.repeat`/`padStart`, `Date.prototype`, `Object.prototype.hasOwnProperty`.
3. **Fix the value/prototype model** — install shared prototypes for `Object`, `Map`, `Set`, `Date`, `String`, and `Function`, and make `new`/prototype lookup work for all built-ins.
4. **Fix bridge host functions** — done; keep host functions aligned if the FFI contract changes.
5. **Keep `runtime.js` compatible** — only rewrite constructs that are cheaper to change in JS than to add to the engine (document any such rewrites).
6. **Verify** — run `cargo test` and the example apps; `simple.js` should work now, the rest after the runtime gaps are closed.

## Boundaries — do not touch

- `src/bridge/` — only add new host-function registrations in `src/main.rs` that call existing bridge methods.
- `src/ink/` — native Ink runtime and Yoga layout.
- `src/render/` — render pipeline.
- `src/compiler/` — compiler output format.
- `src/cli.rs`, `src/signals.rs`, `src/hotreload.rs`.
- **`examples/` are immutable.** If an example fails, fix the runtime; do not edit the example.

Allowed glue points:
- `crates/quench-runtime/src/`
- `src/main.rs` — host-function registration and runtime setup
- `src/event_loop.rs` — JS dispatch calls
- `src/runtime.js` — targeted compatibility rewrites only; do not change example semantics

## Architecture review

The current architecture is a good fit for replacing `rquickjs` with a minimal, Ink-focused runtime:

- **Dedicated crate (`crates/quench-runtime/`)** keeps the engine isolated from the main binary and bridge.
- **swc-based parser/lowering** avoids writing a lexer/parser and gives us TS/JS/TSX support for free.
- **Generic host-function API** lets `src/main.rs` register bridge closures without `quench-runtime` depending on `quench` internals.
- **Shared prototype objects** (started for `Array.prototype`) are the right way to implement JS prototype semantics.

### Known architectural limitations to address

1. **Recursive interpreter** — `eval_expression`/`eval_statement` call themselves recursively. Deeply nested JSX can overflow the Rust stack. The best long-term fix is an iterative interpreter with an explicit evaluation stack, but that is a large rewrite. For now, keep functions small and avoid unbounded recursion in user code.
2. **Monolithic built-ins** — `crates/quench-runtime/src/builtins.rs` mixes Array, Map, Set, Promise, JSON, Math, Object, String, Date, and globals. As it grows it should be split into `builtins/array.rs`, `builtins/map.rs`, `builtins/promise.rs`, etc., which also helps satisfy the build.rs linter.
3. **Value model uses `std::collections::HashMap`** — JS object property enumeration order is not guaranteed by `HashMap`. If `for...in` order or object serialization order becomes observable, switch to `indexmap` or a similar ordered map.
4. **No module system** — `parse_swc` only handles scripts. Compiled TSX with external imports is out of scope for now, but supporting ES modules would require a module loader.
5. **No garbage collector** — values are `Rc<RefCell<...>>` with cycle risk. The current Ink usage does not create obvious cycles, but this should be monitored.

## Tech stack

| Crate | Purpose |
|-------|---------|
| `swc_common`, `swc_ecma_parser`, `swc_ecma_ast`, `swc_atoms` | Parse TS/JS/TSX source via swc. |
| `serde`, `serde_json` | JSON serialization for built-ins and bridge data. |
| `urlencoding` | `encodeURIComponent` / `decodeURIComponent`. |
| `tracing` | Logging. |

The value model currently uses `std::collections::HashMap`. The ordered-map/string-interning/bigint crates discussed earlier are not wired in yet; they can be adopted later if performance or enumeration-order correctness becomes a problem.

## Task index

See `tasks/index.json`.

## Verification

```bash
cargo check
cargo test
cargo run -- examples/simple.js
```

After Tasks 01–04 and 07 are truly complete:

```bash
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark --prop user=admin
cargo run -- examples/animations.tsx
```

## Execution options

1. **Subagent-driven** — one subagent per task with reviews between tasks.
2. **Inline execution** — work through the tasks in this session with checkpoints.
