# Task 21: Strip TypeScript syntax in the lowerer

## Goal

Make the runtime parse and execute `.ts` and `.tsx` source natively by stripping or translating TypeScript-only constructs during lowering. No separate compile step.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/lower/stmt.rs`
- `crates/quench-runtime/src/lower/decl.rs`
- `crates/quench-runtime/src/lower/helpers.rs`
- `crates/quench-runtime/src/ast.rs`

## TypeScript constructs to handle

### Parser

- Use `swc_ecma_parser::Syntax::Typescript(...)` when the input filename ends with `.ts` or `.tsx`.
- Keep `Syntax::Es(...)` for `.js`/`.jsx`.

### Lowering — strip these

- `TsTypeAnn`, `TsTypeParamDecl`, `TsTypeParamInstantiation` — drop type annotations on variables, parameters, functions, classes, and calls.
- `TsAsExpr`, `TsNonNullExpr`, `TsTypeAssertion`, `TsConstAssertion` — lower to the inner expression.
- `TsInterfaceDecl`, `TsTypeAliasDecl` — skip; they have no runtime effect.
- `TsModuleDecl` (namespaces) — lower the body if it contains runtime declarations; skip empty/type-only namespaces.
- `TsImportEqualsDecl` (`import foo = require(...)` / `import foo = Bar`) — lower to a normal variable/import as appropriate.
- `TsExportAssignment` (`export = foo`) — lower to `module.exports = foo`.
- `TsEnumDecl` — lower to a runtime object with reverse mappings, or skip if `--const enum`.
- `TsParameterProperty` (`constructor(public x: number)`) — lower to a parameter plus a `this.x = x` assignment.
- `TsModuleBlock` / `TsNamespaceBody` — flatten into the parent scope.

### Lowering — preserve runtime semantics

- Class fields with type annotations: keep the field and initializer, drop the annotation.
- Generic function/class declarations: keep the declaration, drop the `<T>`.
- `satisfies` operator: lower to the inner expression.
- JSX in `.tsx`: already handled by the existing JSX transform; ensure TypeScript-specific attributes (`type` annotations on props) are dropped.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not introduce a separate compile step or invoke `tsc`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` and `tests/typescript/` are immutable.

## Acceptance criteria

- `ctx.eval_ts("let x: number = 1; x")` returns `1`.
- `ctx.eval_ts("interface Foo {} type Bar = number; let y: Bar = 2; y")` returns `2`.
- `ctx.eval_ts("enum Color { Red, Green }; Color.Red")` returns `0`.
- `ctx.eval_ts("function add(a: number, b: number) { return a + b; }; add(1,2)")` returns `3`.
- A simple `.ts` conformance file parses and runs without `LowerError`.

## Verification

```bash
cargo test -p quench-runtime
```
