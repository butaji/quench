# Task 012: Rust: Hooks Implementation

## Goal
Implement all hooks in Rust and expose to JS via rquickjs.

## Status
> ⚠️ **Partial** - React hooks are stubs, Ink hooks functional
> ⚠️ **Not Integrated** - See Task 009b

## Acceptance Criteria
- [x] `useState<T>(initial)` → `(T, fn(T))` - **stub only**
- [x] `useEffect(deps, fn)` → **stub only (no-op)**
- [x] `useRef<T>(initial)` → `{current: T}` - functional
- [x] `useMemo(fn, deps)` → functional
- [x] `useCallback(fn, deps)` → functional
- [x] `useInput(handler, options)` → functional
- [x] `useApp()` → functional (exit, stdout, stdin, stderr)
- [ ] Hook context stored in Rust `HookContext` - **needs reconciler**

## Code Location
- `src/ink_js.rs::use_state`, `use_effect`, etc.

## Missing: Full React Reconciler
The current implementation lacks a proper React reconciler for:
- `useState` that triggers re-renders
- `useEffect` cleanup and dependency tracking
- Proper hook order enforcement

This requires integrating a reconciler (like `react-reconciler` port to Rust or custom implementation).

## Dependencies
- Task 009b (integration)

## SPEC Reference
§3 Rust Modules; §4 JS Runtime
