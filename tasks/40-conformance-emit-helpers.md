# Task 40: Provide TypeScript emit helpers in conformance context

## Status: COMPLETED

### What was done (2026-06-30)

Added `EMIT_HELPERS` constant to `conformance.rs` with minimal implementations of TypeScript emit helpers:

```rust
const EMIT_HELPERS: &str = r#"
(function(global) {
  global.__extends = ...;
  global.__assign = ...;
  global.__awaiter = ...;
  global.__decorate = ...;
  global.__param = ...;
  global.__metadata = ...;
  global.__importStar = ...;
  global.__importDefault = ...;
  global.__createBinding = ...;
  global.__export = ...;
})(typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : this);
"#;
```

The helpers are prepended to baseline JS in `run_case_with_mode()` so baselines containing `__extends`, `__awaiter`, etc. don't `ReferenceError`.

Added `test_emit_helpers_defined` unit test verifying all helpers are callable:
```rust
ctx.eval("typeof __extends")  // ✓
ctx.eval("typeof __assign")   // ✓
ctx.eval("typeof __awaiter")  // ✓
ctx.eval("typeof __importStar")  // ✓
ctx.eval("typeof __importDefault")  // ✓
```

### Files changed

- `crates/quench-runtime/tests/conformance.rs` — `EMIT_HELPERS`, `get_emit_helpers()`, `test_emit_helpers_defined`

### Verification

```bash
cargo test -p quench-runtime --test conformance test_emit_helpers_defined  # ✓
```
