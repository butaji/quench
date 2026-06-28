# Task 02: Integrate swc parser and lower to runtime AST

## Goal

Use swc to parse JS source, then lower `swc_ecma_ast` into the runtime AST defined in Task 01. Do not write a custom lexer or parser.

## Files

- Create: `src/js_runtime/swc_parse.rs`
- Create: `src/js_runtime/lower.rs`
- Modify: `src/js_runtime/mod.rs` to export `parse`.

## Steps

1. Implement `src/js_runtime/swc_parse.rs`:
   - `pub fn parse_swc(source: &str) -> Result<swc_ecma_ast::Module, ParseError>` using `swc_common::sync::Lrc<SourceMap>`, `swc_common::FileName`, and `swc_ecma_parser::parse_file_as_module` with syntax `Es(EsConfig { jsx: true, ..Default::default() })`.
   - Return a clear error type with line/column information.
2. Implement `src/js_runtime/lower.rs`:
   - `pub fn lower(module: &swc_ecma_ast::Module) -> Result<Program, LowerError>` that walks swc AST nodes and emits the runtime AST from `src/js_runtime/ast.rs`.
   - Map swc expressions, statements, declarations, function/arrow/function expressions, object/array literals, member/call expressions, binary/unary operators, and control flow to the runtime AST.
   - Reject unsupported constructs with a descriptive error instead of silently dropping them.
3. In `src/js_runtime/mod.rs` expose:
   - `pub fn parse(source: &str) -> Result<Program, Error>` that runs `parse_swc` then `lower`.
4. Add parser/lowering unit tests for each supported construct.

## Boundaries

- Work only inside `src/js_runtime/`. Do not touch `src/runtime.js`, the bridge, or the compiler.
- The lowered AST must accept the JS that the compiler currently emits; do not change compiler output to make lowering easier.

## Acceptance criteria

- `cargo test js_runtime::swc_parse` passes.
- `cargo test js_runtime::lower` passes.
- `parse(include_str!("../../runtime.js"))` succeeds, or a documented reduced copy if runtime.js uses unsupported syntax.

## Verification

```bash
cargo test js_runtime::swc_parse js_runtime::lower
```
