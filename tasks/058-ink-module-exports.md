# Task 058: `ink-module-exports` Example — Named/Default/Re-exports, Namespace Imports

**Priority:** P1-High  
**Phase:** 6 — Modules  
**Depends on:** 057

## Problem

Module system is partially tested but lacks an Ink example with varied export patterns.

## Solution

Created a single-file example that demonstrates module-like patterns in TS/TSX:

```tsx
// Single-file example demonstrating:
// - Named imports and exports
// - Default imports and exports
// - Namespace/object imports (simulated)
// - Re-exports (simulated)
```

Note: True multi-file ES module bundling requires additional work in the bundler to strip `export` keywords and handle module hoisting. The current example uses a single-file approach that works in the rquickjs eval context.

## Files Created

- `examples/ink-module-exports/deno.json`
- `examples/ink-module-exports/runts.config.json`
- `examples/ink-module-exports/main.tsx`
- `examples/ink-module-exports/tui/app.tsx`

## Acceptance Criteria

- [x] Example exists with named exports, default export, re-exports, namespace import patterns
- [x] Renders identically in deno and `runts dev` (100% parity)
- [ ] All module patterns produce compilable Rust (partial - patterns demonstrated, multi-file requires bundler work)
- [ ] `runts build --release` produces working binary (depends on compile path support for module patterns)

## Notes

The bundler was refactored to split large files:
- `src/transpile/bundler/mod.rs` (54 lines)
- `src/transpile/bundler/bundler.rs` (221 lines)
- `src/transpile/bundler/imports.rs` (136 lines)
- `src/transpile/bundler/transform.rs` (89 lines)

Total: 500 lines (within limit)
