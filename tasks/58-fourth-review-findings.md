# Task 53: Fourth five-round architecture & code review findings

## Goal

Capture the findings from a fourth set of read-only review rounds, noting what was fixed since Task 52 and what remains.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: fix the correctness blockers that prevent the examples from running end-to-end before optimizing or refactoring for the future.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## What improved since Task 52

- JSX/TSX lowering now works (desugared to `createElement`).
- `switch` fallthrough was added.
- `with` and `do...while` are handled.
- Unary `+` and `delete` are supported.
- `globalThis` is linked to globals.
- ES modules no longer silently drop imports/exports; a module registry was added.
- `Math.random()` range was fixed.
- Function-expression parameters now support destructuring/defaults/rest.

## Rank 1 — End-to-end examples still blocked

1. **Promise `.then`/`.catch`/`all`/`race`/`finally` are still broken or stubs**
   - Files: `builtins/promise.rs:188-218,276-327,347-458`
   - Issue: `all` wraps the input array, `race` never resolves, `catch` resolves with `undefined`, `finally` does not propagate, and resolved promises use an isolated prototype.
   - Fix: Store real reaction lists, resolve iterables in `all`/`race`, propagate settlement, and reuse the registered `Promise.prototype`.

2. **Microtasks are not drained correctly**
   - Files: `src/runtime.js:1236-1281`, `src/bridge/timers.rs:165-167`, `src/event_loop.rs:259-265`
   - Issue: `__ink_enqueue_microtask` only sets a Rust flag; the JS callback is never stored, so `__tb_invoke_microtasks` drains an empty array. `process.nextTick`/`setImmediate` never set the flag.
   - Fix: Store the callback in `microtaskCallbacks` and have `process.nextTick`/`setImmediate` call `__ink_enqueue_microtask`.

3. **Native constructor prototypes are isolated from `Object.prototype`**
   - Files: `builtins/date.rs:59`, `error.rs:25-102`, `function.rs:19`, `numbers.rs:29`, `global.rs:96-186`
   - Issue: `Date`, `Error`, `TypeError`, `ReferenceError`, `SyntaxError`, `Function`, `String`, `Number`, `Boolean`, `Symbol` prototypes have no parent chain.
   - Fix: Create each with `Object::with_prototype(..., object_proto)` and chain error subtypes to `Error.prototype`.

4. **Hot reload does not compile / `--hot` is disabled**
   - Files: `src/event_loop.rs:67-68,315-330`, `Cargo.toml:10`, `src/cli.rs:125-127`
   - Issue: `hotreload` is not a default feature; when enabled, `run_event_loop` creates two mutable borrows of `ctx_wrapper`.
   - Fix: Add `hotreload` to default features, fix the borrow, register bridge functions on the new context, compile TSX before eval, and swap the active context.

5. **`__ink_set_timeout` JSON-stringifies function callbacks**
   - Files: `src/runtime.js:220-226,249-251,1122-1134`
   - Issue: `globalThis.__ink_set_timeout` converts the function argument to a string, so timers registered through the raw bridge never fire. `waitUntilExit` picks this broken wrapper.
   - Fix: Use the `setTimeout` polyfill (`inkSetTimeout`) or make the raw bridge detect and delegate functions.

6. **`setTimeout`/`setInterval` are non-functional stubs**
   - File: `builtins/global.rs:47-60`
   - Issue: They return a dummy handle but never schedule callbacks.
   - Fix: Integrate with the event loop/bridge or remove until implemented.

7. **Real mouse events are never received**
   - File: `src/main.rs:219-229`
   - Issue: `setup_terminal` enables raw mode but never enables crossterm mouse capture.
   - Fix: Push `EnableMouseCapture` on startup and the matching disable sequence in cleanup.

## Rank 2 — Major language / runtime correctness gaps

8. **Assignment / compound-assignment / update re-evaluate the left-hand side**
   - Files: `interpreter/eval_expr/helpers_call.rs:374-476`, `helpers.rs:180-226`
   - Issue: `a[i++] = v` and `a[i++] += 1` read and write at different indices.
   - Fix: Resolve the LHS to a reference once and reuse it.

9. **`Function.prototype.call`/`apply` semantics are broken**
   - File: `interpreter/eval_expr/helpers_call.rs:20-34`
   - Issue: `eval_call_expr` prepends the receiver to args for any method named `call`/`apply`.
   - Fix: Implement real `Function.prototype.call`/`apply` natives.

10. **Numeric-string keys are routed to array storage for all object kinds**
    - File: `value/mod.rs:210-227,254-263`
    - Issue: `obj["123"] = x` expands `elements`/`length` even for ordinary objects.
    - Fix: Only take the numeric-index path when `self.kind == ObjectKind::Array`.

11. **Symbol values are falsy**
    - File: `value/convert.rs:76`
    - Issue: `to_bool` returns `false` for `Value::Symbol`.
    - Fix: Return `true`.

12. **Getters are invoked with the prototype object as `this`**
    - Files: `interpreter/eval_expr/helpers_call.rs:81-100`, `call.rs:292-379`
    - Issue: Property lookup walks the chain and calls getters with the owner object.
    - Fix: Capture the original receiver and pass it as `this`.

13. **Class static members are stored on the instance prototype**
    - Files: `interpreter/eval_expr/helpers_obj.rs:595-618`, `helpers_call.rs:127-169`, `call.rs:336-379`
    - Issue: Static methods are kept as `__static:*` entries on `f.get_prototype()`.
    - Fix: Store static members as own properties of the constructor function.

14. **`instanceof` ignores function / bound-method left operands**
    - File: `interpreter/binary_ops.rs:263-310`
    - Issue: Only `Value::Object` is walked.
    - Fix: Handle functions, native functions, and bound methods by starting from their prototype object.

15. **`for...in` enumerates internal and non-enumerable properties**
    - File: `interpreter/eval_stmt/loops.rs:309-315`
    - Issue: Returns every `properties` key, including internal slots.
    - Fix: Filter to enumerable own string properties.

16. **Object rest elements and destructuring defaults are ignored**
    - Files: `lower/expr.rs:395,561`, `lower/patterns.rs:38,80-89`, `lower/decl_fn.rs:222-235`, `lower/decl_var.rs:204,296-312`
    - Issue: `const {a, ...rest} = obj` drops `rest`; `const {a = 1} = {}` gives `undefined`.
    - Fix: Emit rest-copy helper and nullish-coalescing defaults.

17. **Module import lookup emits invalid optional-chain-free member access**
    - File: `lower/stmt.rs:498-512`
    - Issue: `__moduleRegistry[spec][name] ?? undefined` throws if the module entry is missing.
    - Fix: Emit a null guard before the member access.

18. **Lowering silently swallows subexpression errors**
    - Files: `lower/stmt.rs:70-78,236,643,788`, `lower/expr.rs:102-103,154,622-628`
    - Issue: `filter_map(|x| x.ok())` drops `LowerError`s.
    - Fix: Propagate `Result` and return the first error.

## Rank 3 — Architecture / future alignment

19. **Reactive HIR nodes are still decorative**
    - Files: `ast.rs:247-279`, `interpreter/eval_expr/main.rs`, `lower/*`
    - Issue: `Signal`/`Memo`/`Effect`/`Render` are never emitted or executed.
    - Fix: Implement hook detection and reactive graph evaluator (Task 24), or remove the nodes.

20. **HIR is not A-normal form**
    - Files: `ast.rs:7-16`, `lower/expr.rs:347-352,639-655`
    - Issue: Deeply nested expressions remain.
    - Fix: Add ANF flattening.

21. **Source spans are missing from expressions; `LowerError` lacks location**
    - Files: `ast.rs:183-258`, `lower/mod.rs:60-77`
    - Issue: No file/line/col in diagnostics.
    - Fix: Add `Span` to every node and error.

22. **Public API exposes implementation internals**
    - File: `lib.rs:17-34`
    - Issue: Lowering, builtins, env, interpreter, value internals are `pub`.
    - Fix: Seal to `Context`, `Value`, `Program`, `JsError`, host registration.

23. **Switch lowering uses non-deterministic `SystemTime::now()` labels**
    - File: `lower/stmt.rs:874`
    - Issue: HIR becomes non-reproducible and breaks serialized caches.
    - Fix: Use a monotonic counter.

24. **`parse_swc` uses fragile substring module detection**
    - File: `swc_parse.rs:18-19,32-33,44-45`
    - Issue: `source.contains("import ") || source.contains("export ")` is brittle.
    - Fix: Try `parse_module` first, fall back to `parse_script`.

25. **For-loop initializer only captures the first declarator**
    - File: `lower/stmt.rs:883-900`
    - Issue: `for (let i = 0, j = 0; ...)` drops `j`.
    - Fix: Support multiple declarators or lower to a preceding block declaration.

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
