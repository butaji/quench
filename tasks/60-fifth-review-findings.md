# Task 59: Fifth five-round architecture & code review findings

## Goal

Capture the findings from a fifth set of read-only review rounds. Use this list to track the final correctness and architecture gaps.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: fix the correctness blockers that prevent the examples from running end-to-end before optimizing or refactoring for the future.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Rank 1 — End-to-end examples still blocked

1. **`==` and `!=` are lowered to strict equality**
   - Files: `crates/quench-runtime/src/lower.rs:996-997`
   - Issue: `swc::BinaryOp::EqEq`/`NotEq` map to `BinaryOp::StrictEq`/`StrictNeq`, silently changing JS semantics. The interpreter's abstract-equality arms become dead code.
   - Fix: Map `EqEq`/`NotEq` to `BinaryOp::Eq`/`Neq` and implement full abstract equality.

2. **Template literal interpolations are silently discarded**
   - Files: `crates/quench-runtime/src/lower.rs:907-923`
   - Issue: Only the `quasis` are joined; `` `hello ${name}` `` becomes `"hello"`.
   - Fix: Lower template literals to a left-associative `+` chain of cooked strings and expressions.

3. **Spread elements are lowered incorrectly or rejected**
   - Files: `crates/quench-runtime/src/lower.rs:693-696,800-803,888-892`
   - Issue: Array/call spread flags are ignored; object spread is explicitly rejected. `[...arr]` becomes `[arr]`, `cb(...args)` becomes `cb(args)`.
   - Fix: Preserve a spread flag or lower to helper calls; support object spread.

4. **Rest parameters are silently dropped**
   - Files: `crates/quench-runtime/src/lower.rs:377-383,707-711`
   - Issue: `Pat::Rest` becomes the placeholder `"arg"`/ `""`; excess arguments are lost.
   - Fix: Give rest params a real name and collect excess arguments at call sites.

5. **For-of/for-in with destructuring is rejected**
   - Files: `crates/quench-runtime/src/lower.rs:644-685` (lines 656-662)
   - Issue: `lower_for_head` returns `None` for array/object patterns.
   - Fix: Expand for-head destructuring the same way `VarDeclaration` destructuring is expanded.

6. **Main parser entry only supports plain JS scripts**
   - Files: `crates/quench-runtime/src/swc_parse.rs:16-42`, `crates/quench-runtime/src/lib.rs:128-130`
   - Issue: `Context::parse` uses `Syntax::Es` and `parse_script()`, blocking `.ts`/`.tsx`/JSX/module imports.
   - Fix: Use `Syntax::Typescript` for `.ts`/`.tsx`, enable JSX, and call `parse_module` when imports/exports are present.

7. **Microtask queue is never drained at the JS layer**
   - Files: `src/event_loop.rs:241`, `src/runtime.js:1207-1242`
   - Issue: The loop calls the bridge drain but never invokes `__tb_invoke_microtasks`, so `process.nextTick`, `setImmediate`, and Promise microtasks queued in `runtime.js` never run.
   - Fix: After timer dispatch, call `ctx.call_function("__tb_invoke_microtasks", vec![])?` and loop until empty.

8. **Hot reload rebuilds a context without bridge natives/config**
   - Files: `src/event_loop.rs:274-321`, `src/main.rs:167-531,533-540`
   - Issue: `handle_hot_reload` creates a fresh context but never registers bridge functions or injects the bridge config.
   - Fix: Re-run bridge setup on the new context and swap the active context, or reset/re-evaluate the existing context.

## Rank 2 — Major language / runtime correctness gaps

9. **Function-call scope leak**
   - Files: `crates/quench-runtime/src/interpreter.rs:1042-1103`
   - Issue: `call_value_with_this` pushes a scope but never pops it on early return, so closures accumulate bindings.
   - Fix: Pop the scope in a guard/finally before returning.

10. **`typeof` on undeclared identifiers throws**
    - Files: `crates/quench-runtime/src/interpreter.rs:396-402,1234-1246`
    - Issue: `typeof x` resolves `x` first, so undeclared names throw before `typeof` can return `"undefined"`.
    - Fix: Special-case `typeof <Identifier>` to resolve safely.

11. **Arrow functions bind their own `this`**
    - Files: `crates/quench-runtime/src/interpreter.rs:1047-1049`
    - Issue: Arrow functions should inherit lexical `this`, but `call_value_with_this` sets `this_val` unconditionally.
    - Fix: Skip `set_this` when `f.is_arrow`.

12. **Assignment to undeclared identifier silently creates a local binding**
    - Files: `crates/quench-runtime/src/interpreter.rs:968-985`
    - Issue: `assign_to` falls back to `env.define(...)` in the current scope instead of throwing `ReferenceError` in strict mode or defining on global in non-strict.
    - Fix: Distinguish global assignment from strict-mode errors.

13. **Switch lowering ignores fall-through semantics**
    - Files: `crates/quench-runtime/src/lower.rs:588-618`
    - Issue: `switch` is lowered to nested `if-else`, so case fall-through and shared labels are lost.
    - Fix: Add a `Switch` statement variant or lower to labeled blocks with break targets.

14. **Binding patterns conflated with expressions in the HIR**
    - Files: `crates/quench-runtime/src/ast.rs:91-94`
    - Issue: `ArrayPattern`/`ObjectPattern` live inside `Expression`, forcing the interpreter to handle impossible expression variants.
    - Fix: Introduce a `Pattern` enum and remove patterns from `Expression`.

15. **Source spans are defined but never propagated**
    - Files: `crates/quench-runtime/src/ast.rs:6-10`; `lower.rs` throughout
    - Issue: `Span` exists but no node carries one, so diagnostics have no location.
    - Fix: Add `span: Span` to every HIR node and thread swc spans through lowering.

16. **Silent semantic degradation via pervasive `.ok()` dropping**
    - Files: `crates/quench-runtime/src/lower.rs:262,595-597,695-697,700-702,800-803,814-817`
    - Issue: Unsupported constructs are silently omitted rather than rejected.
    - Fix: Propagate `LowerError` with `?` and reserve `filter_map`/`ok()` only for genuinely optional nodes.

17. **Promise constructor ignores JavaScript function executors**
    - Files: `crates/quench-runtime/src/builtins.rs:863-880`
    - Issue: `new Promise((resolve, reject) => { ... })` is ignored because only `NativeFunction` executors are handled.
    - Fix: Call JS executors via `call_value_with_this(executor, vec![resolve, reject], Value::Undefined)` and reject on throw.

18. **`setTimeout`/`setInterval` accept callbacks but never run them**
    - Files: `crates/quench-runtime/src/builtins.rs:456-466`
    - Issue: Callbacks are stringified and discarded; no timer table or scheduling exists.
    - Fix: Store callbacks in a timer table and invoke them via the host event loop.

## Rank 3 — Architecture / future alignment

19. **Public API exposes too many internals**
    - File: `crates/quench-runtime/src/lib.rs:17-34`
    - Issue: Lowering, swc_parse, builtins, env, interpreter, value internals are all public.
    - Fix: Seal the API to `Context`, `Value`, `Program`, `JsError`, and host registration.

20. **Compiler rewrites hooks/components with unsafe global string replacement**
    - Files: `src/compiler/mod.rs:119-154`
    - Issue: `.replace(hook, ...)` rewrites names inside strings, comments, and object keys.
    - Fix: Use an AST-based transform or scope replacements to free identifiers/call expressions.

21. **Compiler doesn't pin JSX factory/fragment**
    - Files: `src/compiler/mod.rs:31-39,156-172`
    - Issue: esbuild may emit `jsx/jsxs` from `react/jsx-runtime`; `strip_imports` only matches exact `from "react"`/`from "ink"`.
    - Fix: Add `--jsx-factory=ink.createElement --jsx-fragment=ink.Fragment` and strip `react/jsx-runtime` imports.

22. **Native no-op stubs shadow JS event/timer dispatchers**
    - Files: `src/main.rs:496-511`, `src/runtime.js:852-875,1377-1393,1501-1533`
    - Issue: Native stubs for `__tb_dispatch_*`/`__tb_invoke_timers` shadow the real JS implementations if JS definitions cannot replace native globals.
    - Fix: Remove the stub native registrations and let `runtime.js` own dispatch functions.

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
