# Task 015: Architecture Review & Full Ink Parity

**Date:** 2026-06-06
**Status:** In Progress

## Summary

Comprehensive review of runts-ink architecture and ensuring 100% parity across all 3 environments.

---

## Architecture Overview

### Components Structure

```
runts-ink (Rust crate)
├── components.rs    - Box, Text, Newline, Spacer, Static, Transform
├── events.rs       - InputEvent, Key, MouseEvent, FocusId, ResizeEvent
├── flex_layout/    - Yoga/Taffy bridge for flexbox layout
├── js_bridge.rs    - rquickjs FFI for JSX reconciliation
├── props.rs        - Props handling
├── render.rs       - VNode to string rendering
├── style.rs        - BorderStyle, Borders, Display, Overflow, Wrap
└── vnode.rs        - VNode, VNodeContent types
```

### Runts Architecture

```
runts (CLI)
├── hir-render     - Render TSX through HIR runtime
├── dev            - Development server with hot-reload
├── build          - Production build (transpile + compile)
├── codegen        - Generate Rust from TypeScript
└── inspect-hir    - Debug HIR JSON
```

### Three Environment Pipeline

1. **Deno** - Uses npm:ink@7, React 19
2. **runts dev** - HIR runtime with QuickJS
3. **runts build** - Transpile to Rust + compile

---

## Current Status

### Parity Results (Deno vs HIR)
- **Total Examples:** 88
- **Passing:** 88 (100%)
- **Failing:** 0

### Known Gaps

| Feature | Deno | HIR | Compile |
|---------|------|-----|---------|
| useState | ✅ | ✅ (static) | ⚠️ Broken |
| useEffect | ✅ | ⚠️ (once) | ❌ Not implemented |
| useInput | ✅ | ❌ | ❌ Not implemented |
| useFocus | ✅ | ⚠️ (static) | ❌ Not implemented |
| useApp | ✅ | ⚠️ | ❌ Not implemented |
| useStdin | ✅ | ❌ | ❌ Not implemented |

---

## Issues to Fix

### 1. Compile Path with Hooks
The codegen generates Rust code but doesn't properly handle variable scoping with hooks.

```
error[E0425]: cannot find value `count` in this scope
```

**Affected Examples:**
- ink-counter
- ink-hooks
- ink-use-state
- ink-use-effect

### 2. Missing Unit Tests
Need additional coverage for:
- Layout edge cases
- Color parsing edge cases
- Border style edge cases
- Event handling

---

## Task List

| ID | Title | Status |
|----|-------|--------|
| 015-01 | Complete architecture review | In Progress |
| 015-02 | Fix compile path hook handling | Pending |
| 015-03 | Add comprehensive unit tests | Pending |
| 015-04 | Verify all 88 examples parity | Pending |
| 015-05 | Update test harness documentation | Pending |
| 015-06 | Commit and push | Pending |

---

## Files to Modify

| File | Changes Needed |
|------|---------------|
| `crates/runts-ratatui/src/codegen/app.rs` | Hook-aware code generation |
| `crates/runts-ratatui/src/codegen/expr.rs` | Variable reference handling |
| `crates/runts-ratatui/src/codegen/vars.rs` | State variable tracking |
| `crates/runts-ink/src/js_bridge.rs` | Enhanced JS API |
| `run_parity_tests_comprehensive.sh` | Enhanced documentation |
