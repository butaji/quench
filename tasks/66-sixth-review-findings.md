# Task 63: Sixth five-round architecture & code review findings — reduce custom code / unify / simplify

## Goal

Capture the findings from a sixth set of read-only review rounds, this time focused on **reducing custom code, unifying duplicated logic, and replacing hand-rolled pieces with established crates**. Use this list to make the runtime smaller, more correct, and easier to maintain.

## Pareto & reuse note

- **Prefer existing crates, the Rust standard library, and OS features over custom code.** This is the central theme of this review.
- Follow the 80/20 rule: replace the highest-leverage custom subsystems first (parser, regex, JSON, diagnostics, string interning, ordered maps, allocation).
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Rank 1 — Replace custom subsystems with crates

1. **Parser: use `oxc_parser` or a more complete swc-based setup**
   - Files: `crates/quench-runtime/src/swc_parse.rs`, `lower.rs`
   - Issue: The runtime re-implements parser-mode detection with string search, lacks TypeScript/JSX/module support, and maintains a large custom lowerer.
   - Crate options:
     - **`oxc_parser`** — fastest, most conformant JS/TS/JSX parser in Rust; AST is cleaner than swc; transpiler/minifier available.
     - **`swc_ecma_parser`** + `swc_ecma_transforms_*` — already a dependency; use preset-env, TypeScript stripping, JSX transform, module resolver instead of hand-writing them.
   - Fix: Adopt `oxc` (recommended) or extend swc transforms so the custom lowerer only handles the final runtime HIR.

2. **JSON: replace hand-rolled JSON.stringify/parse with `serde_json`**
   - Files: `crates/quench-runtime/src/builtins/json.rs`
   - Issue: Custom JSON conversion is incomplete and duplicated in multiple files.
   - Crate: **`serde_json`** (already used elsewhere).
   - Fix: Implement `Value` → `serde_json::Value` and back; delete custom serialization logic.

3. **Regex: replace ad-hoc regex code with `regress`**
   - Files: `crates/quench-runtime/src/builtins/regexp.rs` or wherever `RegExp` is implemented
   - Issue: A correct ECMAScript regex engine is hard to write.
   - Crate: **`regress`** (pure-Rust ECMAScript-compatible regex) or **`regex`** with ECMAScript semantics mapping.
   - Fix: Delegate `RegExp.prototype.{test,exec,match,replace,split}` to `regress`.

4. **Diagnostics: replace string errors with `miette` or `ariadne`**
   - Files: `crates/quench-runtime/src/value/error.rs`, `lower.rs`, `swc_parse.rs`
   - Issue: Errors are strings; no source spans, no labels, no snippets.
   - Crates: **`miette`** (rich diagnostics, labels, error codes) or **`ariadne`** (beautiful snippets) or **`codespan-reporting`**.
   - Fix: Attach `Span` to HIR nodes and `LowerError`, then render with `miette`.

5. **String interning: use `lasso` or `string-interner`**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `builtins/*.rs`
   - Issue: Property names and identifiers are stored as `String`, causing frequent allocations and slow comparison.
   - Crates: **`lasso`** (single/multi-threaded rodeo) or **`string-interner`**.
   - Fix: Intern all property names/identifiers into `Atom`/`Symbol` and use integer-keyed maps.

6. **Ordered maps: use `indexmap` for object properties and Map/Set order**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `builtins/map.rs`, `builtins/set.rs`
   - Issue: Custom insertion-order tracking is buggy and complex.
   - Crate: **`indexmap`** (already listed in docs).
   - Fix: Use `IndexMap<Atom, Property>` for object properties and `IndexSet` for Set.

7. **BigInt / decimal: use `num-bigint` and `rust_decimal`**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `interpreter/binary_ops.rs`
   - Issue: No `BigInt` support; `Number` is just `f64`.
   - Crates: **`num-bigint`**, **`rust_decimal`**.
   - Fix: Add `Value::BigInt(num_bigint::BigInt)` and use `rust_decimal` if a decimal type is needed.

8. **Allocation: use `bumpalo` for short-lived runtime objects**
   - Files: `crates/quench-runtime/src/interpreter.rs`, `value/mod.rs`, `context/mod.rs`
   - Issue: Frequent small allocations for frames, temporaries, and objects.
   - Crate: **`bumpalo`**.
   - Fix: Arena-allocate call frames, HIR nodes, and temporary object graph during execution.

9. **Fast hashing: use `rustc-hash` or `foldhash` for integer-keyed maps**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `env.rs`
   - Issue: `HashMap` with default hasher is slower than necessary for interned keys.
   - Crates: **`rustc-hash`** (FxHashMap), **`foldhash`**.
   - Fix: Use `FxHashMap`/`foldhash::FastMap` where keys are already hashed or integers.

10. **Errors: use `thiserror` for error enums**
    - Files: `crates/quench-runtime/src/value/error.rs`, `lower.rs`
    - Issue: Error types are hand-written and not `std::error::Error` friendly.
    - Crate: **`thiserror`** (already used in main crate).
    - Fix: Convert `JsError`/`LowerError` to `thiserror` enums.

## Rank 2 — Unify duplicated custom logic

11. **Unify the two call paths (`call_value_with_this` and `Runtime::call_function`)**
    - Files: `crates/quench-runtime/src/interpreter.rs:1042-1103`, `lib.rs:160-185`
    - Issue: `call_function` duplicates call logic without `this` handling, leading to divergent behavior.
    - Fix: Make `call_function` a thin wrapper around `call_value_with_this(..., Value::Undefined)`.

12. **Unify value-to-primitive conversion used by `==`, `+`, and template literals**
    - Files: `interpreter/binary_ops.rs`, `interpreter/eval_expr/helpers.rs`, `value/convert.rs`
    - Issue: Each operator re-implements parts of `ToPrimitive`.
    - Fix: Implement one `to_primitive(input, preferred_type)` and reuse it everywhere.

13. **Unify member access: getter/setter/property lookup is duplicated across call sites**
    - Files: `interpreter/eval_expr/helpers_call.rs`, `interpreter/eval_expr/helpers_obj.rs`, `call.rs`
    - Issue: Multiple functions walk the prototype chain and handle getters differently.
    - Fix: Create a single `get_property(receiver, object, key)` and `set_property(receiver, object, key, value)` that always pass the original receiver.

14. **Unify function/arrow/method parameter binding**
    - Files: `lower.rs`, `interpreter/call.rs`, `env.rs`
    - Issue: Defaults, destructuring, rest, and `arguments` are handled differently for declarations, arrows, and methods.
    - Fix: Build one "bind parameters" routine used by all function calls.

15. **Unify `Program` and module representation**
    - Files: `ast.rs`, `lower.rs`, `interpreter/eval_stmt/mod.rs`
    - Issue: Script vs module is tracked implicitly; imports/exports are special-cased.
    - Fix: Add `Program::Script` and `Program::Module`, and a single `ModuleRecord` loader.

## Rank 3 — Architecture simplifications

16. **Remove or implement the decorative reactive HIR nodes**
    - Files: `ast.rs:247-279`, `interpreter/eval_expr/main.rs`, `runtime.js`
    - Issue: `Signal`/`Memo`/`Effect`/`Render` exist but are never produced/executed.
    - Options:
      - Implement them properly and wire hooks to produce them.
      - Delete them and keep reactivity in `runtime.js` until the HIR is ready.
    - Crate: **`reactive-signals`** or **`futures-signals`** could inspire the graph design, but the runtime semantics are JS-specific so a custom graph is likely required.

17. **Flatten the HIR to A-normal form (ANF)**
    - Files: `ast.rs`, `lower.rs`
    - Issue: Deeply nested expressions make interpretation and future codegen hard.
    - Fix: Add an ANF pass that introduces `Let`/`Temp` bindings.

18. **Seal the public API**
    - File: `lib.rs`
    - Issue: Too many internals are exported.
    - Fix: Export only `Context`, `Value`, `Program`, `JsError`, and host registration traits.

19. **Replace raw-pointer environment with immutable scope frames**
    - Files: `env.rs`, `interpreter.rs`
    - Issue: `push_scope`/`pop_scope` mutate shared `Rc` chains and raw pointers.
    - Fix: Use a `Vec<Frame>` plus parent index ( arena-friendly ); never mutate parent frames.

20. **Replace the compiler's string-based hook/component rewrite with an AST transform**
    - Files: `src/compiler/mod.rs:119-154`
    - Issue: `.replace(hook, ...)` is unsafe and rewrites unrelated identifiers.
    - Crates: **`swc_ecma_transforms`** or **`oxc_transformer`**.
    - Fix: Use a visitor/transformer to rename only free identifiers and call expressions.

## Boundaries

- These findings describe changes in `crates/quench-runtime/src/`, `src/`, and `Cargo.toml`.
- Do not modify `examples/` or `tests/typescript/`.
- Replace subsystems with crates before writing new custom code for the same purpose.

## Verification

After each batch of changes:

```bash
./scripts/run_tests.sh
timeout 60 cargo run -- examples/simple.js
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

All commands must run with a timeout (see Task 31).
