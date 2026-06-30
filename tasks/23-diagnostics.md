# Task 23: Improve diagnostics and error messages

## Status: COMPLETED

### What was done (2026-06-30)

#### Source location tracking in HIR

1. **Added `SourceLocation` struct** to `crates/quench-runtime/src/ast.rs`:
   ```rust
   pub struct SourceLocation {
       pub file: Option<String>,
       pub line: u32,  // 1-indexed
       pub col: u32,   // 0-indexed
   }
   ```
   Implements `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`.

2. **Added `location: Option<SourceLocation>` field** to HIR statement variants:
   - `Stmt::While { condition, body, location }`
   - `Stmt::ForOf { binding, iterable, body, location }`
   - `Stmt::Return(Option<Box<Expression>>, Option<SourceLocation>)`
   - `Stmt::Throw(Box<Expression>, Option<SourceLocation>)`

3. **Thread-local SourceMap for lowering**: Added `CURRENT_SOURCEMAP` thread-local in `lower/mod.rs` with `with_sourcemap()` wrapper. The `span_to_location()` helper converts swc byte offsets to line/col via `SourceMap::lookup_char_pos`.

4. **Updated lowerer**: `lower_script`/`lower_module` now set the thread-local SourceMap before lowering. `lower_stmt` reads the SourceMap and populates `location` on `While`, `ForOf`, `Return`, and `Throw` nodes using swc's per-statement spans.

5. **Added location to `JsError`** in `value/error.rs`:
   - `JsError::WithLocation { message, location }`
   - `JsError::WithTypeAndLocation { error_type, message, location }`
   - Helper constructors: `JsError::with_location()` and `JsError::typed_with_location()`
   - Updated `message()`, `error_type()`, `Display`, and `Debug` implementations

### Regression tests added

Three tests in `crates/quench-runtime/src/context/tests.rs`:
- `test_error_location_tracking` — throw statement produces a caught `JsError`, not a crash
- `test_error_location_in_while_loop` — verifies While nodes carry `Some(location)` with correct line/col
- `test_error_location_return` — verifies Return nodes in function bodies have location info

### Files changed

- `crates/quench-runtime/src/ast.rs` — `SourceLocation`, `location` fields
- `crates/quench-runtime/src/value/error.rs` — `WithLocation`/`WithTypeAndLocation` variants
- `crates/quench-runtime/src/lower/mod.rs` — thread-local SourceMap, `span_to_location`, `with_sourcemap_get_location`
- `crates/quench-runtime/src/lower/stmt.rs` — updated to extract spans and populate `location`
- `crates/quench-runtime/src/lower/decl_fn.rs` — updated to use thread-local SourceMap
- `crates/quench-runtime/src/lower/decl_var.rs` — updated to use thread-local SourceMap
- `crates/quench-runtime/src/lower/expr.rs` — `Return` now takes 2 args
- `crates/quench-runtime/src/swc_parse.rs` — passes `SourceMap` to lowerer via `with_sourcemap`
- `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs` — updated patterns for `Return`/`Throw`/`While`/`ForOf`

### Remaining work (deferred)

- [ ] Runtime stack traces (capturing the full call stack when errors are thrown)
- [ ] Propagating `location` from HIR into `JsError` when eval_stmt throws
- [ ] Pretty-print formatting with source snippets using `miette`/`ariadne`
- [ ] Audit lowering for silent node drops
