# Task 66: Sixth five-round architecture & code review findings — reduce custom code / unify / simplify

## Goal

Capture the findings from a sixth set of read-only review rounds, this time focused on **reducing custom code, unifying duplicated logic, and replacing hand-rolled pieces with the mandatory crates listed below**. These choices are now enforced: any custom code for these subsystems must be removed or justified.

## Pareto & reuse note

- **Prefer existing crates, the Rust standard library, and OS features over custom code.** The crate choices in the table below are mandatory.
- Follow the 80/20 rule: replace the highest-leverage custom subsystems first (JSON, regex, diagnostics, interning, ordered maps).
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Mandatory crate choices

| Subsystem | Mandatory crate(s) | Rationale |
|-----------|--------------------|-----------|
| Parser / TypeScript stripping / JSX transform | `swc_ecma_parser`, `swc_ecma_transforms_*` | Already integrated; use swc transforms instead of hand-rolling TypeScript/JSX lowering. |
| JSON parse/stringify | `serde_json` | Correct, fast, well-tested; delete custom JSON code. |
| ECMAScript regex | `regress` | Pure-Rust ECMAScript-compatible engine. |
| Rich diagnostics | `miette` or `ariadne` | Source spans, labels, snippets; no hand-rolled formatter. |
| String interning | `lasso` | `Rodeo`/`ThreadedRodeo`; property maps become integer-keyed. |
| Ordered maps/sets | `indexmap` / `indexmap::IndexSet` | Deterministic `for...in` order and insertion-ordered Map/Set. |
| BigInt / decimal | `num-bigint` / `rust_decimal` | Standard arbitrary-precision arithmetic. |
| Arena allocation | `bumpalo` | Short-lived frames, temporaries, HIR nodes. |
| Fast integer-keyed maps | `rustc-hash` or `foldhash` | Faster than default hasher for interned keys. |
| Error enums | `thiserror` | Structured, `std::error::Error`-compatible errors. |
| Future AOT/JIT (only when reached) | `cranelift-*` | Smaller/faster to embed than LLVM; do not start with `inkwell`. |

## Rank 1 — Replace custom subsystems with mandatory crates

1. **Parser: use swc transforms, not a custom lowerer**
   - Files: `crates/quench-runtime/src/swc_parse.rs`, `lower.rs`
   - Issue: The runtime re-implements parser-mode detection with string search, lacks TypeScript/JSX/module support, and maintains a large custom lowerer.
   - Fix: Use `swc_ecma_parser` + `swc_ecma_transforms_*` (TypeScript strip, JSX transform, module resolver) and lower only the final runtime HIR.

2. **JSON: replace hand-rolled JSON.stringify/parse with `serde_json`**
   - Files: `crates/quench-runtime/src/builtins/json.rs`
   - Issue: Custom JSON conversion is incomplete and duplicated in multiple files.
   - Fix: Implement `Value` ↔ `serde_json::Value` round-tripping; delete custom serialization logic.

3. **Regex: replace ad-hoc regex code with `regress`**
   - Files: `crates/quench-runtime/src/builtins/regexp.rs` or wherever `RegExp` is implemented
   - Issue: A correct ECMAScript regex engine is hard to write.
   - Fix: Delegate `RegExp.prototype.{test,exec,match,replace,split}` to `regress`.

4. **Diagnostics: replace string errors with `miette` or `ariadne`**
   - Files: `crates/quench-runtime/src/value/error.rs`, `lower.rs`, `swc_parse.rs`
   - Issue: Errors are strings; no source spans, no labels, no snippets.
   - Fix: Attach `Span` to HIR nodes and `LowerError`, then render with `miette` or `ariadne`.

5. **String interning: use `lasso`**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `builtins/*.rs`
   - Issue: Property names and identifiers are stored as `String`, causing frequent allocations and slow comparison.
   - Fix: Intern all property names/identifiers into `lasso` atoms and use integer-keyed maps.

6. **Ordered maps: use `indexmap` for object properties and Map/Set order**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `builtins/map.rs`, `builtins/set.rs`
   - Issue: Custom insertion-order tracking is buggy and complex.
   - Fix: Use `IndexMap<Atom, Property>` for object properties and `IndexSet` for Set.

7. **BigInt / decimal: use `num-bigint` and `rust_decimal`**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `interpreter/binary_ops.rs`
   - Issue: No `BigInt` support; `Number` is just `f64`.
   - Fix: Add `Value::BigInt(num_bigint::BigInt)` and use `rust_decimal` if a decimal type is needed.

8. **Allocation: use `bumpalo` for short-lived runtime objects**
   - Files: `crates/quench-runtime/src/interpreter.rs`, `value/mod.rs`, `context/mod.rs`
   - Issue: Frequent small allocations for frames, temporaries, and objects.
   - Fix: Arena-allocate call frames, HIR nodes, and temporary object graph during execution.

9. **Fast hashing: use `rustc-hash` or `foldhash` for integer-keyed maps**
   - Files: `crates/quench-runtime/src/value/mod.rs`, `env.rs`
   - Issue: `HashMap` with default hasher is slower than necessary for interned keys.
   - Fix: Use `FxHashMap`/`foldhash::FastMap` where keys are already hashed or integers.

10. **Errors: use `thiserror` for error enums**
    - Files: `crates/quench-runtime/src/value/error.rs`, `lower.rs`
    - Issue: Error types are hand-written and not `std::error::Error` friendly.
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
    - Crate inspiration: `reactive-signals` / `futures-signals` for graph design, but JS semantics require a custom graph.

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
    - Fix: Use a `Vec<Frame>` plus parent index (arena-friendly); never mutate parent frames.

20. **Replace the compiler's string-based hook/component rewrite with an AST transform**
    - Files: `src/compiler/mod.rs:119-154`
    - Issue: `.replace(hook, ...)` is unsafe and rewrites unrelated identifiers.
    - Fix: Use `swc_ecma_transforms` visitor/transformer to rename only free identifiers and call expressions.

## Enforcement

- Add the mandatory crate list to `crates/quench-runtime/Cargo.toml`.
- Update `EXECUTE.md` tech stack to match exactly.
- Add a note to `CONTRIBUTING.md` (or `docs/`): a PR that introduces custom code for a mandatory subsystem must first justify why the crate cannot be used.
- Before marking this task done, verify no custom JSON/regex/diagnostic/interning/map-ordering code remains.

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
