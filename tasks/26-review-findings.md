# Task 26: Five-round architecture & code review — ranked findings

## Goal

Capture the findings from five focused read-only review rounds (architecture/HIR, parser/lowering, interpreter/value model, built-ins/host functions, bridge/compiler integration) and turn the highest-impact ones into an actionable, ranked backlog.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: fix the correctness blockers that prevent the examples from running before optimizing or refactoring for the future.
- Defer edge cases, but document them here or spawn a follow-up task so they are not lost.

## Rank 1 — End-to-end interactive examples are blocked

1. **Event-loop dispatch calls registered stubs, not the JS handlers in `runtime.js`**
   - Files: `src/bridge_reg.rs:358-363`, `src/event_loop.rs:128,164,192,263,287`, `src/runtime.js:852,1218,1386,1510,1686`, `crates/quench-runtime/src/context/mod.rs:99-108`
   - Issue: `__tb_dispatch_key`, `__tb_dispatch_mouse`, `__tb_dispatch_resize`, `__tb_invoke_timers` are registered as no-op native stubs, but the real implementations are assigned to `globalThis.__tb_*` in `runtime.js`. `Context::call_function` only looks at the globals `HashMap`, not `globalThis` properties. `__tb_invoke_microtasks` is not registered at all.
   - Fix: Make `Context::call_function` fall back to `globalThis` properties, or change `runtime.js` to bind these as top-level functions.

2. **Promise microtasks are never drained**
   - Files: `src/event_loop.rs:259-294`, `src/runtime.js:1218-1232`, `crates/quench-runtime/src/builtins/promise.rs:10-12,222-230,341-378`
   - Issue: The runtime crate maintains a separate `MICROTASK_QUEUE` for Promise reactions, but the event loop never calls `drain_microtasks()`. `runtime.js` only drains its own `microtaskCallbacks` array.
   - Fix: After each event/timer pass, drain the Promise microtask queue (or unify the two queues).

3. **Hot reload creates and then discards a new context**
   - Files: `src/event_loop.rs:296-348`, `src/main.rs`, `src/bridge_reg.rs`
   - Issue: `handle_hot_reload` builds a new context, loads `runtime.js`, evaluates the file, logs success, and throws the context away. The main loop keeps using the old context. The new context also lacks bridge functions, and raw TSX files fail to parse.
   - Fix: Actually swap the running context, re-register bridge functions, compile TSX/JSX if needed, and refresh `root_id`.

4. **Array.prototype methods return arrays with no prototype**
   - Files: `crates/quench-runtime/src/builtins/array_methods.rs:141,167,247,403,436`, `builtins/array.rs:89-94`, `value/json.rs:64`, `interpreter/call.rs:124,135`
   - Issue: `Object::new_array_from` creates arrays whose `prototype` is `None`, so chaining like `arr.slice().map(...)` fails. Breaks `scroll-view.tsx`, `chat-ui.js`, `log-viewer.tsx`, `multi-select.tsx`, `tabs.js`, `todo-list.js`.
   - Fix: Set `__Array_prototype__` on every array created by builtins, JSON parsing, rest params, and `arguments`.

5. **`==` loose equality is broken for object/function references**
   - File: `crates/quench-runtime/src/interpreter/binary_ops.rs:14-15`
   - Issue: `BinaryOp::Eq` uses `Value == Value`, which returns `false` for all non-primitives. `obj == obj`, `fn == fn`, `[] == []` are all false.
   - Fix: Implement full JS abstract equality (SameValue + coercion + reference equality).

6. **`instanceof` does not walk the prototype chain**
   - File: `crates/quench-runtime/src/interpreter/binary_ops.rs:108-123`
   - Issue: It checks `left.constructor.prototype` pointer equality to `right.prototype` and never walks `left.[[Prototype]]`.
   - Fix: Walk the prototype chain iteratively and compare each link to `right.prototype`.

## Rank 2 — Major correctness / conformance gaps

7. **Unsafe raw-pointer traversal in environment and prototype chains**
   - Files: `crates/quench-runtime/src/env.rs:105-133,147-174,188-216`, `value/mod.rs:166-187,213-228`
   - Issue: `Vec<*const RefCell<…>>` is dereferenced with `unsafe`. This is unnecessary and risks UB if borrows overlap.
   - Fix: Rewrite as safe iterative walks or move to an arena/GcCell layout.

8. **`break` and `continue` are ignored in `for…of` and `for…in`**
   - Files: `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs:30-47,143-162`
   - Issue: Loops only intercept `e.is_return()`; `Break`/`Continue` propagate out unhandled.
   - Fix: Add distinct `JsError::Break`/`JsError::Continue` markers and handle them in every loop form.

9. **Arrow functions bind a fresh call-site `this` instead of lexical `this`**
   - File: `crates/quench-runtime/src/interpreter/call.rs:110-111`
   - Issue: `call_js_function` unconditionally calls `set_this(this_val)`. Arrow functions should ignore it.
   - Fix: Skip `set_this` when `f.is_arrow` and resolve `this` from the captured closure chain.

10. **`String.prototype.split` returns an empty array**
    - File: `crates/quench-runtime/src/interpreter/call.rs:355-363`
    - Issue: It creates an array of the right length but never populates `elements`.
    - Fix: Assign each part into `elements`.

11. **Object spread copies internal Map/Set slots**
    - File: `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs:241-252`
    - Issue: Spread copies implementation-internal keys such as `_map_*` / `_set_*`.
    - Fix: Copy only enumerable own string/Symbol properties; add an `enumerable` flag to descriptors or maintain an internal deny-list.

12. **No expression-level recursion limit**
    - File: `crates/quench-runtime/src/interpreter/mod.rs:33-76`, `call.rs:51`
    - Issue: `EVAL_DEPTH` is only checked in `call_js_function`. Deep nested expressions can still overflow the Rust stack.
    - Fix: Move depth checking into `eval_expression`/`eval_statement` or convert to a trampoline.

13. **No JS error taxonomy or stack traces**
    - Files: `crates/quench-runtime/src/value/error.rs:12-21`, `interpreter/eval_stmt/mod.rs:181-183`
    - Issue: `JsError` is just `Regular(String) | StackOverflow | Return`. No `TypeError`/`ReferenceError`, no line/col, no frames.
    - Fix: Add error variants with message + stack frames; emit proper error types.

14. **Native functions cannot access `this`**
    - Files: `crates/quench-runtime/src/interpreter/call.rs:57-60`, `context/mod.rs:91-96`
    - Issue: `Context::register_native` accepts `Fn(Vec<Value>)`; `this` is stored in a thread-local that is not exported.
    - Fix: Change the native-function trait to receive `this: Value` explicitly.

15. **Getters/setters are not inherited through the prototype chain**
    - File: `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs:330-364,503-514`
    - Issue: Only the direct object is checked for `__getter:prop` / `__setter:prop`.
    - Fix: Walk the prototype chain for getter/setter metadata before ordinary access.

16. **`Function.prototype.call` / `apply` are special-case syntax hacks**
    - File: `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs:123-139`
    - Issue: `fn.call(...)` is detected at the call site and rewritten. It fails for dynamic property access, bound methods, etc.
    - Fix: Implement real `Function.prototype.call` / `apply` methods on `FUNCTION_PROTOTYPE`.

17. **`var` re-assignment may write to the wrong scope**
    - Files: `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs:49-66`, `env.rs:147-174`
    - Issue: `VarKind::Var` uses `Environment::set`, which walks up and writes to the first binding found.
    - Fix: Resolve `var` declarations to the nearest function/global scope at declaration time.

18. **`break` / `continue` statements are silently dropped during lowering**
    - Files: `crates/quench-runtime/src/lower/stmt.rs:42-75`
    - Issue: `swc::Stmt::Break` and `swc::Stmt::Continue` are not matched and fall to `_ => None`.
    - Fix: Add `Statement::Break(Option<String>)` / `Statement::Continue(Option<String>)` and lower them.

19. **Class declarations are silently erased**
    - File: `crates/quench-runtime/src/lower/decl.rs:526-533`
    - Issue: `swc::Decl::Class(_)` hits the catch-all `_ => None`.
    - Fix: Lower classes to constructor/prototype objects or emit a clear `LowerError`.

20. **ESM `import` / `export` are silently dropped**
    - Files: `crates/quench-runtime/src/lower/stmt.rs:31-39`
    - Issue: `ImportDecl`, `ExportNamedDecl`, `ExportDefaultDecl` all return `None` except `ExportDefaultExpr`.
    - Fix: Lower imports to variable assignments from a module object, or error explicitly.

## Rank 3 — Real-example support gaps

21. **Date.now static method missing**
    - File: `crates/quench-runtime/src/builtins/date.rs:25`
    - Issue: `Date` is a bare `NativeFunction`; `Date.now()` does not exist.
    - Fix: Wrap `Date` in a callable object with a `now` static method.

22. **Number.prototype.toFixed / number primitive member dispatch missing**
    - Files: not implemented; `interpreter/eval_expr/helpers_call.rs:18-24`
    - Issue: `(n).toFixed(1)` returns `undefined`.
    - Fix: Add `Number.prototype` linkage and implement `toFixed`, `toPrecision`, `toExponential`.

23. **Date.prototype.toTimeString missing**
    - File: `crates/quench-runtime/src/builtins/date.rs`
    - Issue: `now.toTimeString().slice(...)` is used in several examples but undefined.
    - Fix: Add `Date.prototype.toTimeString`.

24. **Array.from does not support iterables or array-like objects**
    - File: `crates/quench-runtime/src/builtins/array.rs:56-95`
    - Issue: Only Set/Map are detected via internal prefixes. `Array.from({ length: 8 })` returns `[]`.
    - Fix: Implement `Symbol.iterator` traversal and a length-indexed fallback.

25. **Math.sin / cos / tan / log / exp etc. missing**
    - File: `crates/quench-runtime/src/builtins/math.rs:22-48`
    - Issue: Only a handful of Math functions exist; `use-animation.tsx` needs `Math.sin`.
    - Fix: Add standard trig/log/exp/atan functions.

26. **String / Number / Boolean constructors lack `prototype` linkage**
    - File: `crates/quench-runtime/src/builtins/global.rs:74-96`
    - Issue: They are bare `NativeFunction`s, so `new String(...)` does not inherit `String.prototype`.
    - Fix: Wrap each constructor in an object with `__call` and a `prototype` property.

27. **Object.keys / values / entries ignore array elements**
    - File: `crates/quench-runtime/src/builtins/object.rs:14-49`
    - Issue: They iterate only `properties`, so `Object.keys([1,2,3])` returns `["length"]`.
    - Fix: Include indexed elements when the target is an array.

28. **Set insertion order is broken**
    - Files: `crates/quench-runtime/src/builtins/map.rs:410-440,499-521,454-497`
    - Issue: `add` stores `_order_<key>` but never appends to `_insertion_order`, which is always empty.
    - Fix: Maintain a single `_insertion_order` array and update it in `add`, `delete`, `clear`.

29. **Map/Set `for…of` iteration has ordering and staleness bugs**
    - Files: `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs:67-123`, `builtins/map.rs:20-46`
    - Issue: `for…of` iterates the underlying `HashMap` keys directly, ignoring insertion order and deleted entries.
    - Fix: Use the same insertion-order metadata as the prototype methods and fix the Set prefix.

30. **Spread syntax only expands Arrays, not generic iterables**
    - Files: `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs:284-292,453-471`
    - Issue: `[...iterable]` and `fn(...iterable)` only spread `ObjectKind::Array`.
    - Fix: Reuse the iterable-extraction logic for spreads.

31. **Compiler SHIMS reassign a `const` array**
    - File: `src/compiler/mod.rs:183-281`
    - Issue: `const __tb_keypress_handlers = []` is later reassigned.
    - Fix: Change the declaration to `let`.

32. **Compiler breaks multi-line `import { … } from 'ink'`**
    - Files: `src/compiler/mod.rs:93-117,164-180`
    - Issue: Imports are processed line-by-line.
    - Fix: Parse imports as a unit or transform before esbuild.

33. **`runtime.js` overwrites native bridge functions with slow wrappers**
    - Files: `src/bridge_reg.rs:41-356`, `src/runtime.js:65-258`
    - Issue: `runtime.js` shadows native implementations with `globalThis.__ink_create_root = function() { __ink_call(...) }`. The `__ink_call_fast` fast path is unused.
    - Fix: Decide on a single bridge layer and remove the redundant indirection or use the fast path.

## Rank 4 — Architecture / future alignment

34. **The “runtime AST” is a direct JS AST, not the documented HIR**
    - Files: `crates/quench-runtime/src/ast.rs`, `lower/*.rs`
    - Issue: The IR retains nested JS expressions and conventional statements. It is not ANF, effect-explicit, or reactive as documented.
    - Fix: Redesign `ast.rs` as a true HIR with `Let`/`Temp` bindings, atom operands, reactive nodes, and swc-free public API.

35. **Reactive primitives are completely absent**
    - File: `crates/quench-runtime/src/ast.rs`
    - Issue: No `Signal`, `SignalGet`, `SignalSet`, `Memo`, `Effect`, `Render` nodes exist.
    - Fix: Add them and detect hooks/components during lowering.

36. **swc-specific leaks in the public API**
    - Files: `crates/quench-runtime/src/lower/mod.rs`, `lower/*.rs`, `lib.rs`
    - Issue: Lowering functions are `pub` and take `&swc::...` types.
    - Fix: Hide lowering behind `pub(crate)` and expose only `parse_swc` / `Context::eval`.

37. **No source spans in errors**
    - Files: `crates/quench-runtime/src/lower/mod.rs:23-41`, `ast.rs:5-16`
    - Issue: `Span` only stores byte offsets; `LowerError` is just a `String`.
    - Fix: Include file/line/col in `Span` and thread spans into `LowerError`.

38. **Parser has no TypeScript syntax configured**
    - Files: `crates/quench-runtime/src/swc_parse.rs:47-57,84-88,114-118`
    - Issue: All entry points use `Syntax::Es(...)`. Direct `.ts`/`.tsx` input fails.
    - Fix: Add `Syntax::Typescript(...)` path for `.ts`/`.tsx`.

39. **Many lowering paths silently swallow errors**
    - Files: throughout `lower/`
    - Issue: `lower_*` functions return `Option<...>` and turn failures into `None`.
    - Fix: Propagate `Result<..., LowerError>` so unsupported constructs report precise messages.

## Boundaries

- These findings describe what needs to be fixed in `crates/quench-runtime/src/`, `src/`, and `Cargo.toml`.
- Do not modify `examples/` or `tests/typescript/`.
- Fix the Rank 1 and Rank 2 items before touching Rank 4 architecture work.

## Verification

After each batch of fixes:

```bash
cargo test -p quench-runtime
cargo run -- examples/simple.js
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
```
