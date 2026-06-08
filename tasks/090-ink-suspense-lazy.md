# Task 090: `ink-suspense-lazy` Example — `Suspense`, `lazy`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`React.Suspense` and `React.lazy` are standard patterns for code splitting and async component loading. No existing Ink example exercises these React APIs.

## Solution

Created example at `examples/ink-suspense-lazy/` demonstrating:
- Suspense component placeholder
- Lazy-loaded component pattern (simplified for TUI)
- Border and styled components

Added to React shim (`src/transpile/js_bundle/react_shim.rs`):
- `Suspense` function - renders children immediately (no actual suspense in TUI)
- `lazy` function - simplified implementation for synchronous loading

## Output

```
React Suspense + Lazy Example

╭──────────────────────────────────────────────────────────────────────────────╮
│This is loaded lazily!                                                        │
╰──────────────────────────────────────────────────────────────────────────────╯
This text appears immediately.
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-suspense-lazy/`
- [x] Uses Suspense component structure
- [x] Uses lazy component pattern
- [x] Renders identically in deno and `runts dev`
- [x] Parity harness passes with 100% match

## Notes

- Full async `lazy(() => import(...))` with Promise is not yet implemented
- The React shim provides stub implementations that work for TUI (no actual async loading)
- For TUI use cases, components are typically loaded synchronously anyway
- Suspense fallback is not shown since we don't have actual async loading
