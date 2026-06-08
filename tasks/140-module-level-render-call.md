# Task 140: Handle `render()` Call Inside Imported Module

**Priority:** P1-High
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example calls `render()` at the module level:

```tsx
// tui/app.tsx
render(React.createElement(App));
```

This conflicts with our project convention where `main.tsx` imports `App` and calls `render(<App />)`.

When `runts dev` evaluates the bundle:
1. The module-level `render()` runs during eval
2. Then `__runts_render_with_effects({})` is called, which may re-render
3. This could cause double-render or the module-level render may not return a VNode the bridge expects

## Strategy Options

**Option A:** Detect module-level `render()` calls and skip them (replace with no-op during transpile).
**Option B:** Support module-level `render()` as the primary entry point (don't require main.tsx).
**Option C:** Document that examples must use `main.tsx` entry point and `export default App`.

## Acceptance Criteria

- [ ] `../tui1` renders correctly whether `render()` is at module level or in main.tsx
- [ ] No double-render artifacts
- [ ] `--once` mode captures the correct static frame
- [ ] Document the convention clearly in `EXECUTE.md`
