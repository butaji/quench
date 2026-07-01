# Task 51: Second five-round architecture & code review findings

## Goal

Capture the findings from a second set of read-only review rounds run after the first big implementation push. Use this list to update priorities and make sure tasks/docs reflect the current code reality.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: fix the correctness blockers that prevent the examples from running end-to-end before optimizing or refactoring for the future.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Rank 1 — End-to-end examples still blocked

1. **`useApp()` is called inside event handlers, but it is a render-only hook**
   - Files: `src/runtime.js:460-467`, `examples/counter.js:20`, `examples/use-bridge.tsx:12`, `examples/animations.tsx:90`
   - Issue: `useApp` uses `getHookState`, which throws when `currentInstance` is `null` outside render. All interactive examples call `useApp().exit()` inside `useInput` handlers.
   - Fix: Make `useApp` return a stable object without hook state, or rewrite examples to destructure `useApp()` at component top level.

2. **CLI `--prop` values are parsed but never injected into the JS runtime**
   - Files: `src/main.rs:174-181`, `src/cli.rs:179-185`
   - Issue: `cli_args.bridge_config.props` is built but `inject_bridge_config` receives `BridgeConfig::default()`.
   - Fix: Pass the parsed `BridgeConfig` into `init_runtime`/`setup_runtime`.

3. **Hot reload creates and discards a new context**
   - Files: `src/event_loop.rs:298-349`
   - Issue: `handle_hot_reload` builds a new context, loads `runtime.js`, evaluates the file, and throws the context away. It also omits bridge function registration.
   - Fix: Register bridge functions on `new_ctx`, replace the active context, and re-inject bridge config.

4. **Promise `.then`/`.catch` handlers registered before resolution are silently dropped**
   - Files: `crates/quench-runtime/src/builtins/promise.rs:222-230`, `234-269`
   - Issue: `enqueue_promise_reactions` stores `Value::Undefined` callbacks; when the promise resolves the queued jobs have no handlers.
   - Fix: Store real `onFulfilled`/`onRejected` reactions on the promise object and enqueue them on resolve/reject.

5. **Microtasks are not drained by the event loop**
   - Files: `src/runtime.js:1237-1251`, `src/event_loop.rs:259-265`, `crates/quench-runtime/src/builtins/promise.rs:340-378`
   - Issue: `process.nextTick`/`setImmediate` polyfills push callbacks but never set the Rust flag; the bridge microtask queue discards the callback; the runtime Promise queue is never drained by the loop.
   - Fix: Unify microtask handling or drain all three queues each tick.

6. **`new Date()` returns `undefined`**
   - Files: `crates/quench-runtime/src/builtins/date.rs:13-27`
   - Issue: Date constructor returns `Value::Undefined` unconditionally.
   - Fix: Set `_timestamp` on the constructed object and return `this_val`; as a function return a string.

7. **`Array.from` does not support array-like objects or mapping function**
   - Files: `crates/quench-runtime/src/builtins/array.rs:56-105`
   - Issue: `Array.from({length: N})` returns empty array and the second argument is ignored.
   - Fix: Read `length` property, create indexed elements, and call the mapping function if provided.

8. **ES module `import`/`export` declarations are silently dropped**
   - Files: `crates/quench-runtime/src/lower/stmt.rs:37-70`
   - Issue: Import/export declarations fall through to `None`, so `.tsx` examples that start with `import … from 'ink'` lose their imports.
   - Fix: Lower imports to require/module assignments and preserve exports.

9. **JSX/TSX nodes are rejected during lowering**
   - Files: `crates/quench-runtime/src/lower/expr.rs:56-60`
   - Issue: Every JSX node errors out even though the parser accepts TSX.
   - Fix: Lower JSX to `React.createElement`/`jsx` calls or to the Ink component convention.

## Rank 2 — Major language / runtime correctness gaps

10. **Environment `pop_scope` mutates shared `Rc` closure environments**
    - Files: `crates/quench-runtime/src/env.rs:274-314`, `crates/quench-runtime/src/value/function.rs:28`
    - Issue: `push_scope`/`pop_scope` mutate the captured parent chain in place, so closures observe post-pop state.
    - Fix: Make scope chains immutable; `push_scope` creates a new frame without mutating parents.
    - **Note**: `set_var` was also fixed to correctly assign to the immediate parent scope instead of walking up the entire chain.

11. **Thread-local `RETURN_VALUE` / `THROWN_VALUE` hold only one value**
    - Files: `crates/quench-runtime/src/value/error.rs:9-16`, `222-276`
    - Issue: Nested try/catch/re-throw can overwrite payloads; typed errors cannot carry thrown objects.
    - Fix: Embed payloads in enum variants or use per-frame value slots.

12. **Getter `this` binding uses the prototype object, not the original receiver**
    - Files: `crates/quench-runtime/src/interpreter/eval_expr/helpers_call.rs:81-170`
    - Issue: Walked prototype getters receive the prototype object as `this`.
    - Fix: Pass the original receiver through the prototype walk.

13. **`globalThis` is disconnected from actual globals**
    - Files: `crates/quench-runtime/src/context/mod.rs:52-55`, `86-89`
    - Issue: `globalThis.Array`/`console` are undefined even though bare `Array`/`console` resolve.
    - Fix: Define globals as properties on `globalThis` and resolve identifiers against it.

14. **`object_to_primitive` in `==`/`+` uses an empty environment**
    - Files: `crates/quench-runtime/src/interpreter/binary_ops.rs:147-182`
    - Issue: `valueOf`/`toString` are looked up against `Environment::new()` and never found.
    - Fix: Look them up on the object's prototype chain and call with the object as `this`.

15. **`instanceof` walk starts at the object itself instead of its prototype**
    - Files: `crates/quench-runtime/src/interpreter/binary_ops.rs:196-244`
    - Issue: Walk compares `left` to `right.prototype` first; per spec it should start at `left.[[Prototype]]`.
    - Fix: Start at `o.borrow().prototype.clone()`.

16. **`var` assignment targets the outermost scope that owns the name**
    - Files: `crates/quench-runtime/src/env.rs:182-230`
    - Issue: `var` can assign to a block-scoped `let` or miss a function-scoped `var`.
    - Fix: Track function/global vs block scope frames and assign only in the first function/global frame.

17. **Optional chaining duplicates the base expression**
    - Files: `crates/quench-runtime/src/lower/expr.rs:646-712`
    - Issue: `a?.b` evaluates `a` twice; nested chains multiply side effects.
    - Fix: Bind the base to a fresh temp variable and reference the temp.

18. **Destructuring rest/default/computed keys are dropped or miscompiled**
    - Files: `crates/quench-runtime/src/lower/patterns.rs`, `lower/decl_fn.rs`, `lower/expr.rs`, `lower/stmt.rs`
    - Issue: Rest patterns return empty arrays; defaults fall through; for-of/in heads ignore complex patterns.
    - Fix: Expand rest with slice/object-rest; expand defaults with nullish-coalescing; support patterns in loop heads.

19. **Computed property names are unsupported or replaced by placeholder**
    - Files: `crates/quench-runtime/src/lower/helpers.rs:19-26`, `lower/decl.rs:455`, `lower/expr.rs:927`
    - Issue: `extract_prop_key` returns `""` for computed keys; classes use `"[computed]"`.
    - Fix: Support `PropertyKey::Computed` in objects, classes, and destructuring.

20. **Multiple built-in prototypes are not chained to `Object.prototype`**
    - Files: `builtins/global.rs`, `builtins/numbers.rs`, `builtins/date.rs`, `builtins/function.rs`, `builtins/error.rs`
    - Issue: String/Number/Boolean/Symbol/Date/Function/Error prototypes have no `[[Prototype]]`.
    - Fix: Create all built-in prototypes with `Object::with_prototype(..., object_prototype)`.

## Rank 3 — Architecture / future alignment

21. **Reactive HIR nodes exist but are never produced or executed**
    - Files: `ast.rs:226-258`, `lower/expr.rs:592-602`, `interpreter/eval_expr/main.rs:82-87`
    - Issue: `Signal`/`Memo`/`Effect`/`Render` are in the enum but the lowerer never emits them and the interpreter errors.
    - Fix: Implement hook/component detection and a reactive execution engine (Task 22 + Task 24).

22. **The "HIR" is still a conventional nested JS AST, not ANF**
    - Files: `ast.rs:176-220`, `lower/expr.rs`
    - Issue: Complex expressions are deeply nested; no `Let`/`Temp` nodes.
    - Fix: Add ANF flattening during lowering.

23. **swc-specific types leak through the public lowering API**
    - Files: `lower/mod.rs`, `lower/helpers.rs`, etc.
    - Issue: Lowering helpers are `pub` and expose `swc_atoms::Atom`, `swc::Expr`, etc.
    - Fix: Make lowering `pub(crate)` and expose only `Context::parse` / `Context::eval`.

24. **Public API surface exposes too many internals**
    - Files: `lib.rs:17-34`, `builtins/*`, `env.rs`
    - Issue: The crate advertises a runtime boundary but leaks parser, lowerer, builtins, environment.
    - Fix: Restrict `lib.rs` to `Context`, `Value`, `Program`, `HostFunctions`, and `register_native`.

25. **No module HIR; ESM is silently dropped**
    - Files: `ast.rs:55-57`, `lower/stmt.rs:37-70`
    - Issue: `Program` only has `Script`; module items vanish.
    - Fix: Add `Program::Module` or emit clear errors.

26. **Lowering silently swallows unsupported constructs**
    - Files: throughout `lower/`
    - Issue: Many lowerers return `Option` and use `filter_map`, dropping failures.
    - Fix: Convert lowering to `Result<..., LowerError>` and propagate failures with spans.

27. **Source spans are insufficient for diagnostics**
    - Files: `ast.rs:21-25`, `lower/mod.rs:60-77`
    - Issue: `Span` only has byte offsets; `LowerError` is a plain string.
    - Fix: Extend `Span` with file/line/col and attach it to `LowerError`.

28. **Effects and mutations are implicit, not explicit HIR nodes**
    - Files: `ast.rs:195`, `lower/expr.rs:354-381`
    - Issue: Mutations are ordinary `Assignment` expressions, not effect nodes.
    - Fix: Split assignment into statement-level effect nodes.

## Boundaries

- These findings describe what needs to be fixed in `crates/quench-runtime/src/`, `src/`, and `Cargo.toml`.
- Do not modify `examples/` or `tests/typescript/`.
- Fix Rank 1 and Rank 2 correctness issues before returning to Rank 3 architecture work.

## Verification

After each batch of fixes:

```bash
./scripts/run_tests.sh
cargo run -- examples/simple.js
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
```

All commands must run with a timeout (see Task 31).
