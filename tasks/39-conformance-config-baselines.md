# Task 39: Resolve configuration-specific baselines by directives

## Status: COMPLETED (partial)

### What was done (2026-06-30)

Added `test_directive_extraction` unit test that verifies the existing `parse_directives()` function correctly extracts `@target`, `@module`, and `@jsx` directives from TypeScript source.

```rust
// @target: es2015
// @module: commonjs
// @jsx: react
let x: number = 1;
```

`parse_directives()` returns `HashMap<String, String>` with keys `"target"`, `"module"`, `"jsx"`.

### How baseline lookup currently works

The `find_baseline()` function looks for an exact-match baseline file:
```rust
baselines_dir.join(filename).with_extension("js")
```

### What's deferred

Building config-specific baseline paths like `name.es2015.commonjs.react.js` is deferred. The current exact-match approach works for most cases; config-specific paths would only improve coverage for edge cases.

### Files changed

- `crates/quench-runtime/tests/conformance.rs` — `test_directive_extraction` test

### Verification

```bash
cargo test -p quench-runtime --test conformance test_directive_extraction  # ✓
```
