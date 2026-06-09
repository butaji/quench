# Task 009b: Integrate ink_js.rs into main.rs

## Status: ✅ Done

## What Was Done

### 1. Integrated `ink_js.rs` into `main.rs`
- Added `mod ink_js;` to `main.rs`
- `ink_js::register(ctx)` is called during rquickjs context setup
- `register()` exposes component tags (`Box`, `Text`, etc.) and the `ink` namespace

### 2. Removed old JS shims
- Deleted `src/js/ink.js` (455 lines)
- Deleted `src/js/host-config.js` (103 lines)
- Deleted `dist/ink.js` (455 lines)
- Deleted `src/js/runtime.js` (moved to `src/runtime.js`)
- Deleted `src/js/` directory entirely

### 3. Cleaned up `main.rs`
- Removed the massive inline JS eval (~376 lines of stubs and wrappers)
- Kept the `__ink_call` native Rust function (the single FFI entry point)
- `runtime.js` now contains the `__ink_*` bridge wrappers and is self-contained

### 4. Runtime architecture after integration
```
main.rs
├── ink_js::register(ctx)      # Native constants + namespace
├── __ink_call native fn       # Single Rust FFI entry point
├── runtime.js (include_str!)  # Reconciler + bridge wrappers
└── user JS code
```

## Pragmatic Decision: Keep runtime.js

The full React reconciler (useState, useEffect, mountTree, reconcileTree) remains in
`src/runtime.js` (~950 lines including bridge wrappers).  Moving it entirely to Rust
is tracked as **Task 054 (Rust Reconciler)**.

Keeping runtime.js is the pragmatic choice because:
1. It provides working hooks and reconciliation today
2. The event loop depends on `__tb_dispatch_key` / `__tb_dispatch_mouse` defined in it
3. Timer callbacks are currently strings returned from Rust; runtime.js manages the JS-side registry
4. Removing it would require completing Task 053 (Function callbacks) and Task 054 (Reconciler)

## Acceptance Criteria

- [x] `main.rs` imports `ink_js` module
- [x] `ink_js::register()` called during rquickjs context setup
- [x] No `--bundle` flag needed for basic examples — ink globals come from runtime.js
- [x] Removed `src/js/*.js`
- [x] Removed `dist/ink.js`
- [x] `examples/simple-hello.js` works without `--bundle`
- [x] `examples/counter.js` works (uses runtime.js reconciler)
- [x] All existing tests pass
- [x] No regression in existing functionality

## Code Location
- `src/ink_js.rs` — Native rquickjs module (constants + namespace)
- `src/main.rs` — Calls `ink_js::register()`, loads `runtime.js`
- `src/runtime.js` — Canonical JS runtime (reconciler + bridge wrappers)

## Dependencies
- Task 009, 010, 011 (code integrated)
- Task 012 (hooks now functional via runtime.js)

## Related Future Work
- **Task 053**: Replace string callbacks with rquickjs Function refs (performance)
- **Task 054**: Move reconciler from runtime.js into Rust (architecture alignment with SPEC)
