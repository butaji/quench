# Task 302: Fix constructor property assignment issue

## Status: in_progress

## Priority: P1 correctness

## Problem

TSX examples like `use-bridge.tsx` and `animations.tsx` crash with:
```
RENDER ERROR: undefined undefined undefined
MOUNT: output.type = undefined
```

## Root Cause

Bug in hook system (`runtime.js`): `useMemo` and `useCallback` don't properly handle nested hook calls.

When `useMemo(() => { /* hook calls */ }, [])` is called:
1. `getHookState()` returns hook at index N and increments `currentHookIndex` to N+1
2. But `fn()` is called AFTER, using the same `currentHookIndex` context
3. Any hooks inside `fn()` use the same slot index, causing slot collision

Example with `useBridge()`:
- `useBridge()` → `useMemo(() => ({...}), [])` → slot 0 consumed
  - Inside fn: `useApp()` → `useMemo(() => ({...}), [])` → **slot 0 collision!**
  - Result: `useBridge` hook slot gets `useApp`'s value

## Fix

In `useMemo` and `useCallback`, save/restore `currentHookIndex` around `fn()` call:

```javascript
function useMemo(fn, deps) {
  const state = getHookState();
  const hookIdx = currentHookIndex - 1;  // Save index after getHookState
  if (state.type === 'empty') {
    state.type = 'memo';
    // Save current index, call fn (which may have nested hooks), restore
    const savedIdx = currentHookIndex;
    state.value = fn();
    state.deps = deps;
    currentHookIndex = savedIdx + 1;  // Account for this hook's slot
  } else {
    const oldDeps = state.deps;
    const hasChanged = !deps || !oldDeps || deps.length !== oldDeps.length ||
      deps.some((d, i) => d !== oldDeps[i]);
    if (hasChanged) {
      // Same pattern for updates
      const savedIdx = currentHookIndex;
      state.value = fn();
      state.deps = deps;
      currentHookIndex = savedIdx + 1;
    }
  }
  return state.value;
}
```

## Acceptance Criteria

- [ ] `use-bridge.tsx` runs without "RENDER ERROR"
- [ ] `animations.tsx` runs without "RENDER ERROR"
- [ ] `counter.js` continues to work (regression test)
- [ ] Add scenario test for nested hooks in useMemo
- [ ] All existing tests pass

## Test Files

- `crates/quench-runtime/tests/scenarios.rs` - add nested hooks test
- Manual test: `cargo run -- examples/use-bridge.tsx --prop theme=dark`
