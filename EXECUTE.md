# Goal: Finish the custom TS/JS/TSX runtime and make it fully Ink-compatible

`quench-runtime` already exists as a dedicated workspace crate under `crates/quench-runtime/`, and `rquickjs` has been removed from the main crate. The remaining work is to close the interpreter gaps so that `runtime.js`, TSX/JSX source, all Ink examples, and a meaningful subset of the TypeScript conformance test suite run correctly without touching the bridge, renderer, compiler, or native Ink runtime.

> **One-line rule:** We write the **execution engine** (parser lowering, value model, eval loop, built-ins, host-function glue). We do **not** write a parser — **swc** parses JS/TS/TSX. We use crates for parsing; standard-library objects are implemented in Rust because they are the engine's observable JS environment.

## Principles

1. **Reuse before rewriting.** Prefer existing crates, the Rust standard library, and OS features over custom implementations. Examples: `swc` for parsing, `serde_json` for JSON, `regex` for text matching, `indexmap` if order matters, `num-bigint` for BigInt, `regress` for regex, `bumpalo`/`mimalloc` for memory. Only write custom code when no suitable crate exists or when the feature is the runtime's unique value.
2. **80/20 Pareto rule.** Target the ~20% of features that unblock ~80% of the examples and conformance tests first. Edge cases and full spec compliance are explicitly deferred but must be documented in the task notes — never silently ignored.
3. **Work example-to-example.** Prioritize gaps based on what the next failing example actually needs, rather than implementing features in abstract spec order.
4. **Document deferrals.** Every postponed feature gets a note or a dedicated follow-up task so it is not forgotten.
5. **Clear diagnostics.** Every parser, lowering, runtime, and bridge error must tell the user what went wrong, where (file/line/column), and what to do about it. No silent drops, no raw panics, no obscure internal names exposed to users.
6. **Test-driven development.** Every bug fix, feature, and refactor starts with a failing unit test. We follow the red-green-refactor cycle: write the test, watch it fail for the expected reason, write the minimal code to pass, then refactor while keeping tests green. No production code without a failing test first.
7. **Comprehensive unit-test coverage.** Runtime behavior must be pinned with unit tests in `crates/quench-runtime/tests/` (and `tests/` for the main crate) before it is considered done. Integration tests run the example apps; unit tests exercise edge cases that the examples do not hit.

## Test-driven development process

All work in `crates/quench-runtime/` follows TDD:

1. **Red** — write a minimal unit test that demonstrates the missing behavior or bug.
2. **Verify red** — run the test and confirm it fails for the expected reason.
3. **Green** — write the smallest amount of code that makes the test pass.
4. **Verify green** — run the test suite and confirm the new test passes without breaking existing tests.
5. **Refactor** — clean up duplication, improve names, split functions; keep tests green.
6. **Repeat** for the next behavior.

### Unit-test requirements

- Every bug fix from Task 26 must be accompanied by a regression test.
- Every new language feature (optional chaining, destructuring, class, async, modules, etc.) must have unit tests covering the happy path and representative edge cases.
- Built-ins must be tested against the ECMAScript-style expectations used by the TypeScript conformance suite where possible.
- Tests live in `crates/quench-runtime/tests/` and mirror the module structure of the source.
- Examples (`examples/`) are integration tests, not replacements for unit tests.

### Test commands

```bash
# Run the runtime unit tests
cargo test -p quench-runtime

# Run integration/parity tests
cargo test

# Run a specific conformance category (when harness exists)
cargo test -p quench-runtime --test conformance -- expressions
```

## Current state

- `crates/quench-runtime/` has a working skeleton: swc parser, runtime AST, interpreter, value/object model, and built-ins.
- Major modules have been split into subdirectories: `builtins/`, `lower/`, `interpreter/`, `value/`, `context/`, `eval_stmt/`.
- What works at a basic level:
  - Parser/lowering: computed member access, template literal expressions, `for...of`/`for...in`, object/array spread, rest parameters in function declarations, nullish coalescing (`??`), `in`, `instanceof`, getters/setters, module/script fallback, destructuring declarations.
  - Built-ins: most `Array.prototype` methods, `Map`/`Set` constructors and prototype methods, `String.prototype` methods (`repeat`, `padStart`, etc.), `Date.prototype` getters and `toLocaleTimeString`, `Object.prototype`, real `JSON.parse`, native `Error` constructors, global `Infinity`/`NaN`/`undefined`.
  - Interpreter: recursion-depth guard, rest-param binding, `New` expression wiring, bound methods, spread expansion in calls/literals, getter/setter invocation, `typeof` on undeclared identifiers.
  - Bridge/event loop: `__ink_get_node_parent`, `__ink_set_raw_mode`, `__ink_stdin_is_raw`, populated `__ink_get_node_children`, JSON props serialization, hot-reload re-registration.
  - Compiler: hooks are no longer incorrectly prefixed inside `createElement` calls.
  - Build hygiene: `build.rs` lints `src/` and `crates/quench-runtime/src/` but only panics for `src/compiler/` violations; other modules get warnings.
- Major gaps and blockers are documented in **Task 26**. The highest-impact ones are:
  - Event-loop dispatch calls registered stubs instead of `runtime.js` handlers; Promise microtasks are never drained.
  - Hot reload creates a new context and discards it.
  - Array instances from builtins lack `Array.prototype`; `String.prototype.split` returns empty arrays.
  - `==` and `instanceof` are wrong for objects/functions.
  - `break`/`continue` are dropped or ignored; arrow functions use call-site `this`.
  - Optional chaining, class lowering, async/await, import/export, `delete`/unary `+` are missing.
  - `Date.now`, `Number.prototype.toFixed`, `Date.prototype.toTimeString`, trig/log Math functions, iterable `Array.from`, Set insertion order are missing/broken.
  - `build.rs` does not yet fail the build on every `*.rs` lint violation.
- `examples/simple.js` may work; `counter.js`, `use-bridge.tsx`, and `animations.tsx` are blocked until the Rank 1/2 gaps are fixed.

## Approach

1. **HIR-first design** — `swc` parses TS/JS/TSX and the lowerer strips types to produce a **high-level, unified HIR**. The interpreter executes the HIR today; a future AOT compiler can consume the same HIR.
2. **Fix parser/lowering bugs** — support module/script parsing, computed member access, template literal expressions, optional chaining, rest parameters, `for...of`/`for...in`, nullish coalescing, getters/setters.
3. **Complete standard-library objects** — real mutating `Array.prototype`, `Map`/`Set` methods and iterators, `Promise`, `String.prototype.repeat`/`padStart`, `Date.prototype`, `Object.prototype.hasOwnProperty`.
4. **Fix the value/prototype model** — install shared prototypes for `Object`, `Map`, `Set`, `Date`, `String`, and `Function`, and make `new`/prototype lookup work for all built-ins.
5. **Fix bridge host functions** — done; keep host functions aligned if the FFI contract changes.
6. **Keep `runtime.js` compatible** — only rewrite constructs that are cheaper to change in JS than to add to the engine (document any such rewrites).
7. **Verify** — run `cargo test` and the example apps; `simple.js` should work now, the rest after the runtime gaps are closed.

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

The current architecture is a good fit for replacing `rquickjs` with a minimal, Ink-focused runtime, and it is designed to evolve into an AOT-compilable engine:

- **Dedicated crate (`crates/quench-runtime/`)** keeps the engine isolated from the main binary and bridge.
- **swc-based parser/lowering** avoids writing a lexer/parser and gives us TS/JS/TSX support for free.
- **HIR layer** — the lowerer produces a single high-level, language-agnostic, **functional and reactive** IR. It is consumed by the interpreter today and can be consumed by a future AOT compiler without re-parsing source.
- **Generic host-function API** lets `src/main.rs` register bridge closures without `quench-runtime` depending on `quench` internals.
- **Shared prototype objects** (started for `Array.prototype`) are the right way to implement JS prototype semantics.

### HIR design: functional + reactive

The HIR is intentionally designed to make both interpretation and future AOT compilation efficient:

- **Functional core.** The IR is expression-oriented and close to A-normal form (ANF): most operations produce a single value bound to a name, and values are immutable by default. This makes constant folding, dead-code elimination, common-subexpression elimination, and inlining straightforward.
- **Explicit effects.** Mutations (`Assign`, property set, `delete`), I/O, and rendering are explicit effect nodes. Pure sub-expressions can be memoized or reordered; effect nodes anchor sequencing.
- **Reactive primitives.** The HIR includes first-class nodes for the reactive primitives that Ink/Quench actually uses:
  - `Signal { id, initial }` — a mutable reactive cell (e.g., `useState`).
  - `Memo { deps_expr, compute }` — a cached derived value (e.g., `useMemo`).
  - `Effect { deps_expr, callback }` — a scheduled side effect (e.g., `useEffect`).
  - `Render { component, props }` — a reactive render boundary.
- **Dependency tracking.** Each `Memo`/`Effect`/`Render` node declares its dependencies explicitly. At runtime the engine builds a reactive graph so that when a `Signal` changes, only the memos, effects, and components that actually depend on it are re-evaluated.
- **Component functions are pure functions of props + signals.** A component call becomes a `Render` node whose body is a pure function plus an explicit dependency list. This lets the AOT backend compile components to efficient reactive code and lets the interpreter skip unchanged subtrees.
- **Closures and control flow.** Functions, arrow functions, and closures are first-class HIR values. `if`, loops, `try/catch`, `break`, `continue`, and `return` are represented explicitly so the interpreter can execute them and the AOT backend can lower them to branches or CPS without re-parsing source.
- **Optimization-friendly shape.** Because the HIR is high-level but side-effect-free except at explicit nodes, the AOT backend can:
  - inline small pure functions,
  - fold constants across pure operations,
  - hoist memos out of render loops,
  - eliminate unused signals and dead branches.

### Known architectural limitations to address

1. **Recursive interpreter** — `eval_expression`/`eval_statement` call themselves recursively. Deeply nested JSX can overflow the Rust stack. The best long-term fix is an iterative interpreter with an explicit evaluation stack, but that is a large rewrite. For now, keep functions small and avoid unbounded recursion in user code.
2. **Monolithic built-ins** — `crates/quench-runtime/src/builtins.rs` mixes Array, Map, Set, Promise, JSON, Math, Object, String, Date, and globals. As it grows it should be split into `builtins/array.rs`, `builtins/map.rs`, `builtins/promise.rs`, etc., which also helps satisfy the build.rs linter.
3. **Value model uses `std::collections::HashMap`** — JS object property enumeration order is not guaranteed by `HashMap`. If `for...in` order or object serialization order becomes observable, switch to `indexmap` or a similar ordered map.
4. **No module system** — `parse_swc` only handles scripts. Compiled TSX with external imports is out of scope for now, but supporting ES modules would require a module loader.
5. **No garbage collector** — values are `Rc<RefCell<...>>` with cycle risk. The current Ink usage does not create obvious cycles, but this should be monitored.
6. **Primitive boxing** — `Value` is not `Copy`; primitives are heap-allocated. This is acceptable for correctness work but should be replaced with NaN boxing or tagged pointers before the runtime is considered production-ready.

## Tech stack

| Crate | Purpose |
|-------|---------|
| `swc_common`, `swc_ecma_parser`, `swc_ecma_ast`, `swc_atoms` | Parse TS/JS/TSX source via swc. |
| `serde`, `serde_json` | JSON serialization for built-ins and bridge data; HIR caching for AOT. |
| `urlencoding` | `encodeURIComponent` / `decodeURIComponent`. |
| `tracing` | Logging. |
| `thiserror` / `miette` / `ariadne` | Structured errors and rich source diagnostics. |
| `cranelift` / `cranelift-module` / `cranelift-object` / `cranelift-jit` | Future AOT/JIT code generation (preferred over LLVM for size and speed of compilation). |
| `inkwell` | Alternative LLVM bindings if Cranelift is insufficient later. |
| `lasso` / `string-interner` | String interning for property names and identifiers. |
| `indexmap` | Ordered object property maps with deterministic iteration. |
| `bumpalo` | Arena allocation for short-lived HIR nodes, frames, and render trees. |
| `regress` / `regex` | ECMAScript regex engine. |
| `num-bigint` / `rust_decimal` | `BigInt` and decimal arithmetic. |

The value model currently uses `std::collections::HashMap`. The ordered-map/string-interning/bigint crates discussed earlier are not wired in yet; they can be adopted later if performance or enumeration-order correctness becomes a problem.

## Performance roadmap

Once the runtime is functionally correct, the following interpreter-level optimizations are the biggest wins for a pure AST interpreter (no JIT/AOT required):

1. **NaN-boxed / tagged `Value`** — make `Value: Copy` and 64-bit. Pack `f64`, object pointers, string pointers, and tags (`null`, `undefined`, `true`, `false`, int31) into a single `u64`. Avoid `Box<Value>`/`Rc<Value>` for primitives.
2. **String interning** — convert every identifier and property name into a `u32` atom. Use `lasso` (`Rodeo`/`ThreadedRodeo`) or `string-interner`. Object property maps then become `HashMap<Atom, Value>`/`IndexMap<Atom, Value>` with integer hashing.
3. **Object shapes (hidden classes) + inline caches** — attach a `ShapeId` to every object; cache `(expected_shape, offset)` on hot AST nodes (member expressions, calls, identifiers) so the fast path is an indexed array access instead of a hash lookup.
4. **Slot-indexed environments** — run a scope-analysis pass before execution, assign each local a stack slot (`u32`), and store locals in a dense `Vec<Value>` per frame. No HashMap lookups for variable access.
5. **Arena allocation** — use `bumpalo` for call frames, temporary eval state, and short-lived objects. Consider `mimalloc` or `jemalloc` as the global allocator.
6. **Explicit evaluation stack / trampoline** — replace recursive `eval_expression`/`eval_statement` with an explicit stack of frames to avoid native stack overflow, improve cache locality, and enable `try/catch/finally` and generators.
7. **Faster maps and regex** — use `rustc-hash`/`foldhash` for atom-keyed maps; use `regress` for a pure-Rust ECMAScript regex engine and `num-bigint` for `BigInt`.

### Future AOT/JIT direction (out of scope for now)

Once the HIR interpreter is fast enough and Ink apps run smoothly, the next long-term step is native code generation. The research points to **Cranelift** (`cranelift-module`, `cranelift-object`, `cranelift-jit`) as the best first backend — smaller and faster to embed than LLVM/`inkwell`. The HIR is intentionally designed so it can be consumed directly by a future AOT compiler without a separate bytecode layer.

For the current phase, focus only on interpreter-level optimizations and a clean HIR. Do not add a bytecode VM or Cranelift backend yet.

### Performance crates to evaluate

| Crate | Purpose |
|-------|---------|
| `nanbox` or hand-rolled | NaN-boxed / tagged `Value` representation. |
| `lasso` | String interning (single-threaded `Rodeo`, multi-threaded `ThreadedRodeo`). |
| `string-interner` | Alternative interner with `DefaultSymbol`/`serde` support. |
| `compact_str` / `smol_str` | Small-string optimized non-interned payloads. |
| `indexmap` | Ordered property storage with `rustc-hash` hasher. |
| `rustc-hash` / `foldhash` | Fast HashMap for integer (atom) keys. |
| `bumpalo` | Arena allocation for frames and temporary objects. |
| `smallvec` | Stack-allocated argument/local vectors. |
| `regress` | Pure-Rust ECMAScript regex engine. |
| `num-bigint` | `BigInt` implementation. |
| `mimalloc` / `tikv-jemallocator` | High-performance global allocator. |
| `cranelift` / `cranelift-module` / `cranelift-object` / `cranelift-jit` | Bytecode-to-native code generation for JIT and AOT. |
| `inkwell` | LLVM bindings if Cranelift is insufficient. |

> **Note:** A fully optimized AST interpreter is still expected to be 10–30× slower than a bytecode VM. The leap to bytecode + ICs is the first major performance milestone; AOT/JIT come after that.

## Review findings

A five-round read-only review (architecture/HIR, parser/lowering, interpreter/value model, built-ins/host functions, bridge/compiler integration) produced a ranked list of issues. The full list is in **Task 26**. The highest-impact blockers are:

1. **Event-loop dispatch calls stubs**, not the real `runtime.js` handlers, and Promise microtasks are never drained.
2. **Hot reload creates a new context and discards it** instead of swapping the running context.
3. **Array instances created by builtins lack `Array.prototype`**, breaking chained array methods.
4. **`==` and `instanceof` are incorrect** for objects and do not walk the prototype chain.
5. **Unsafe raw-pointer traversal** in environment and prototype chains.
6. **`break`/`continue` are dropped or ignored** in lowering and loops.
7. **Arrow functions use call-site `this`**, not lexical `this`.
8. **Missing built-ins**: `Date.now`, `Number.prototype.toFixed`, `Date.prototype.toTimeString`, trig/log Math functions, iterable `Array.from`, `Object.keys` for arrays, broken Set insertion order.
9. **Compiler issues**: `const` reassignment in SHIMS, multi-line import handling.
10. **Architecture drift**: the current "runtime AST" is a conventional JS AST, not the documented functional + reactive HIR.

Fix Rank 1 and Rank 2 correctness issues before returning to HIR architecture work.

## Task index

See `tasks/index.json`.

## Verification

```bash
cargo check
cargo test
cargo run -- examples/simple.js
```

After Task 09 is complete, `cargo build` must fail if any `*.rs` file exceeds 500 lines, any function body exceeds 40 lines, or any function exceeds cyclomatic complexity 10.

After Tasks 01–04, 07, and 14 are truly complete:

```bash
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark --prop user=admin
cargo run -- examples/animations.tsx
```

## TypeScript conformance test suite

To measure runtime correctness beyond Ink examples, the official TypeScript repo is added as a shallow submodule at `tests/typescript/`.

- Use `tests/typescript/tests/cases/conformance/**/*.ts` as the corpus.
- Parse each `.ts` file **directly** in `quench-runtime` using TypeScript syntax mode and strip type annotations during lowering. No separate compile step.
- Execute the resulting runtime AST directly in the interpreter.
- Record pass/fail per category (expressions, statements, functions, classes, iterators, modules, async).
- Fix failures by implementing the missing runtime feature or adding type-stripping support in the lowerer; do not modify the TypeScript fixtures.

The target is **not** to pass 100% of conformance tests (many are type-check only). The target is to pass all runtime-relevant cases and to use failures as a prioritized backlog.

### Verification for conformance

```bash
git submodule update --init tests/typescript
cargo test -p quench-runtime --test conformance -- --test-threads=1
```

## Execution options

1. **Subagent-driven** — one subagent per task with reviews between tasks.
2. **Inline execution** — work through the tasks in this session with checkpoints.
