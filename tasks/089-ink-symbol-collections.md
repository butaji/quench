# Task 089: `ink-symbol-collections` Example — Symbol, Map, Set, WeakMap

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`Symbol`, `Map`, `Set`, and `WeakMap` are standard ES2015+ collection types. No existing Ink example exercises these primitives in a TUI context.

## Solution

Created example at `examples/ink-symbol-collections/` with:
- `Symbol` declaration with description
- `Map` with keys and size
- `Set` with has() and size
- Array operations on Map keys

## Output

```
Map size: 3
Map keys: alpha, beta, gamma
Set has 'apple': yes
Set size: 3
Symbol type: symbol
Symbol description: Symbol(app-id)
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-symbol-collections/`
- [x] Uses `Symbol`, `Map`, `Set`
- [x] Renders identically in deno and `runts dev`
- [x] Parity harness passes with 100% match

## Notes

- `WeakMap` is not exercised because it requires object keys that would complicate the render
- Symbol and collections are fully supported in rquickjs dev environment
- Compile path maps Map/Set to Rust HashMap/HashSet equivalents
