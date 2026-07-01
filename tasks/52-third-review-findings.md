# Task 52: Third five-round architecture & code review findings

## Goal

Capture the findings from a third set of read-only review rounds. Use this list to update priorities after the most recent implementation push.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: fix the correctness blockers that prevent the examples from running end-to-end before optimizing or refactoring for the future.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Rank 1 — End-to-end examples still blocked

1. **Reactive HIR nodes are decorative; hooks run entirely in `runtime.js`**
   - Files: `crates/quench-runtime/src/ast.rs:226-258`, `interpreter/eval_expr/main.rs:82-87`, `src/runtime.js:337-409`
   - Issue: `Signal`/`Memo`/`Effect`/`Render` exist in the enum but the interpreter rejects them and the lowerer never emits them. `useState`/`useMemo`/`useEffect` are ordinary calls to `runtime.js` functions.
   - Fix: Implement hook detection in the lowerer and a reactive graph evaluator (Task 24), or remove the inert HIR nodes until they are real.

2. **JSX/TSX is parsed but rejected during lowering**
   - Files: `crates/quench-runtime/src/lower/expr.rs:56-60`, `swc_parse.rs:16-50`
   - Issue: `JSXElement`, `JSXFragment`, `JSXMember`, etc. return `LowerError("JSX not supported")`. This blocks every `.tsx` example.
   - Fix: Lower JSX to `createElement`/h calls or to native `Render` HIR nodes.

3. **ES module `import`/`export` declarations are silently dropped**
   - Files: `crates/quench-runtime/src/lower/stmt.rs:37-71`
   - Issue: `ImportDecl`, `ExportNamedDecl`, `ExportAllDecl`, and `ExportDefaultDecl` fall through to `None`. `export const`/`function`/`class` are also dropped.
   - Fix: Add module-statement HIR variants and a module loader, or at minimum emit a `LowerError` instead of dropping.

4. **`Math.random()` returns values in ~[0, 0.233) instead of [0, 1)**
   - File: `crates/quench-runtime/src/builtins/math.rs:100-103`
   - Issue: Divides nanoseconds by `u32::MAX`. Random dashboards/animations are heavily biased.
   - Fix: Use `rand::thread_rng().gen::<f64>()`.

5. **Promise `.then`/`.catch` / `Promise.all` / `Promise.race` / `finally` are broken or stubs**
   - Files: `crates/quench-runtime/src/builtins/promise.rs:186-323`, `382-424`
   - Issue: Reactions registered before resolution are dropped; `all` is a stub; `race` never resolves; `finally` does not propagate settlement; resolved promises use an ad-hoc prototype.
   - Fix: Store real reactions, implement iterable resolution, and reuse the real `Promise.prototype`.

6. **Microtasks (`process.nextTick`, `setImmediate`, Promise reactions) are not drained**
   - Files: `src/event_loop.rs:259-265`, `src/runtime.js:1236-1281`
   - Issue: JS-side callbacks are enqueued but `__ink_enqueue_microtask` is never called from JS; the event loop only drains when the bridge explicitly flags it.
   - Fix: Call `__ink_enqueue_microtask` from `process.nextTick`/`setImmediate`, or drain all queues each tick.

7. **Hot reload creates a fresh context without bridge functions and does not replace the active context**
   - Files: `src/event_loop.rs:298-336`
   - Issue: `handle_hot_reload` builds a new context, loads `runtime.js`, evaluates raw TSX, and discards the context.
   - Fix: Register bridge functions, compile TSX before eval, and swap the event loop's context.

8. **Compiler SHIMS overwrite the runtime.js `process` polyfill and break stdout/exit**
   - Files: `src/compiler/mod.rs:262-280`, `src/runtime.js:1253-1282`
   - Issue: `globalThis.process` is redefined unconditionally, `process.stdout.write` calls a non-existent `ink.stdout_write`, and `process.exit` discards exit codes.
   - Fix: Guard the shim with `if (!globalThis.process)` or remove the redundant definition.

## Rank 2 — Major language / runtime correctness gaps

9. **Recursive interpreter has no uniform stack-depth guard**
   - Files: `interpreter/mod.rs:33-76`, `interpreter/eval_expr/main.rs:15-30`, `interpreter/call.rs:52-57`
   - Issue: `MAX_EVAL_DEPTH` is only checked in `call_function_value`; deep expressions/loops/native calls can overflow the Rust stack.
   - Fix: Add depth checks to all recursive eval paths or convert to an explicit eval stack.

10. **`in` operator operands are swapped**
    - File: `crates/quench-runtime/src/interpreter/binary_ops.rs:185-192`
    - Issue: It stringifies the object and checks the key as an `Object`, so `'x' in obj` is always false.
    - Fix: Stringify `left` and check membership on `right`.

11. **`object_to_primitive` in `==`/`+` evaluates methods in an empty environment**
    - File: `crates/quench-runtime/src/interpreter/binary_ops.rs:147-182`
    - Issue: `valueOf`/`toString` are looked up and called via `Identifier` in a fresh empty scope, so they are never found.
    - Fix: Call the methods directly on the object as `this`.

12. **Native constructor prototypes are isolated from `Object.prototype`**
    - Files: `value/function.rs:211-222`, `builtins/date.rs:58`, `error.rs:25-102`, `function.rs:19`, `global.rs:79-114`, `numbers.rs:12`
    - Issue: `Date`, `Error`, `TypeError`, `Function`, `String`, `Number`, `Boolean` prototypes have no parent chain.
    - Fix: Set each built-in prototype's `prototype` to `__Object_prototype__` (and chain error subtypes to `Error.prototype`).

13. **Environment uses raw pointers into `RefCell`s and mutates closure scopes in place**
    - Files: `env.rs:107-135`, `149-176`, `183-230`, `244-272`, `274-314`
    - Issue: `push_scope`/`pop_scope` restructure the parent chain with `std::mem::replace` while raw pointers may be live; `var` resolution walks root-to-current and assigns to the first existing name.
    - Fix: Use immutable scope frames and proper function-vs-lexical scope tagging.

14. **`instanceof` walk starts at the object itself**
    - File: `crates/quench-runtime/src/interpreter/binary_ops.rs:229-235`
    - Issue: `current` is initialized to `o` instead of `o.prototype`.
    - Fix: Start at the object's `[[Prototype]]`.

15. **Class static members are stored on the instance prototype, not the constructor**
    - Files: `interpreter/eval_expr/helpers_obj.rs:534-557`, `helpers_call.rs:127-169`, `call.rs:336-379`
    - Issue: Static methods are stored as `__static:<name>` on `f.get_prototype()`.
    - Fix: Store static members as own properties on the constructor function.

16. **Array elements are duplicated in `properties` and `elements`**
    - File: `crates/quench-runtime/src/value/mod.rs:205-251`
    - Issue: `Object::set` writes numeric indices to both; reads come from `properties`.
    - Fix: Route numeric keys only to `elements`.

17. **`switch` fallthrough is lost**
    - File: `crates/quench-runtime/src/lower/stmt.rs:275-303`
    - Issue: `switch` is lowered to nested if-else; cases cannot fall through.
    - Fix: Add a `Switch` statement node or lower to a loop with fallthrough tracking.

18. **Function-expression parameters lose destructuring, defaults, and rest**
    - File: `crates/quench-runtime/src/lower/expr.rs:94-106`
    - Issue: Non-identifier params become the placeholder `"arg"`; defaults/destructuring/rest are ignored.
    - Fix: Reuse the arrow/function-declaration parameter expansion.

19. **Object-pattern rest elements and defaults are ignored or wrong**
    - Files: `lower/patterns.rs:38`, `decl_var.rs:204-235`, `decl_fn.rs:150-164-234`, `expr.rs:299-312`
    - Issue: Object rest returns empty; array rest assigns `source[i]` instead of `source.slice(i)`; defaults are not applied.
    - Fix: Implement object-rest copying, array slicing, and nullish-coalescing default assignments.

20. **`with` and `do...while` are silently dropped**
    - File: `crates/quench-runtime/src/lower/stmt.rs:170,197`
    - Issue: Both return `None`.
    - Fix: Add AST/runtime support or emit explicit `LowerError`s.

21. **Lowering silently swallows failures via `filter_map(|x| x.ok())`**
    - Files: `lower/stmt.rs:12-26`, `lower/expr.rs:87-92,604-612,622-627`, and others
    - Issue: A failing subexpression is omitted instead of propagating an error.
    - Fix: Convert lowering to `Result` and return the first error.

22. **`delete` and unary `+` are rejected**
    - File: `crates/quench-runtime/src/lower/helpers.rs:134-145`
    - Issue: Both operators cause `LowerError`.
    - Fix: Add them to the AST and runtime, or desugar.

23. **`globalThis` is disconnected from the real globals**
    - Files: `context/mod.rs:17-20,52-55,86-89`
    - Issue: `globalThis.Array`/`console` are undefined even though bare `Array`/`console` resolve.
    - Fix: Make `globalThis` the root environment object.

## Rank 3 — Architecture / future alignment

24. **HIR is not actually A-normal form**
    - Files: `lower/expr.rs:746-804,640-712`, `lower/stmt.rs:275-303`
    - Issue: Deeply nested `Binary`, `Conditional`, and `Sequence` nodes remain.
    - Fix: Add an ANF flattening pass.

25. **Source spans are missing from most expressions and `LowerError`**
    - Files: `ast.rs:176-258`, `lower/mod.rs:60-77`
    - Issue: `Expression` variants have no span; `LowerError` is just a `String`.
    - Fix: Attach `Span` to every node and error.

26. **Public API exposes too many internals**
    - File: `crates/quench-runtime/src/lib.rs:17-34`
    - Issue: Lowering, swc_parse, builtins, env, interpreter, value internals are all public.
    - Fix: Seal the API to `Context`, `Value`, `Program`, `JsError`, and host registration.

27. **No dedicated `Pattern` type; patterns live in `Expression`**
    - File: `ast.rs:206-208`
    - Issue: `ArrayPattern`/`ObjectPattern` are expression variants even though they are only valid in binding/assignment contexts.
    - Fix: Introduce a `Pattern` type and use it in declarations, assignments, and for-heads.

28. **Program has no module variant**
    - File: `ast.rs:54-57`
    - Issue: `Program` is only `Script(Vec<Statement>)`.
    - Fix: Add `Program::Module`.

29. **`parse_swc` uses string search for module detection**
    - File: `swc_parse.rs:16-26,30-39,42-51`
    - Issue: Checks `source.contains("import ") || source.contains("export ")`, which is fragile.
    - Fix: Try `parse_module` first and fall back to `parse_script`.

30. **Destructure temporaries use unhygienic names**
    - Files: `lower/expr.rs:388`, `decl_var.rs:123`, `patterns.rs:64`
    - Issue: Names like `__destructure_temp` and `__arr_src_0` can collide with user code.
    - Fix: Generate unique temps with a monotonic counter.

## Boundaries

- These findings describe what needs to be fixed in `crates/quench-runtime/src/`, `src/`, and `Cargo.toml`.
- Do not modify `examples/` or `tests/typescript/`.
- Fix Rank 1 and Rank 2 correctness issues before returning to Rank 3 architecture work.

## Verification

After each batch of fixes:

```bash
./scripts/run_tests.sh
timeout 60 cargo run -- examples/simple.js
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

All commands must run with a timeout (see Task 31).
