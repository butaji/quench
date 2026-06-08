# Task 135: Add `Date` Object to rquickjs Bridge

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example creates `Date` instances repeatedly:

```tsx
const timestamp = new Date().toLocaleTimeString("en-US", {
  hour: "2-digit", minute: "2-digit", hour12: false
});
```

`Date` may or may not exist in rquickjs. If it does not, the example crashes with `ReferenceError: Date is not defined`.

## Bridge Implementation

Option A: Use rquickjs built-in `Date` if available.
Option B: Polyfill `Date` with Rust-backed implementation:

```rust
let date_ctor = Function::new(ctx.clone(), || {
  // Return current timestamp as string or object
})?;
// Add prototype methods: toLocaleTimeString, toISOString, etc.
globals.set("Date", date_ctor)?;
```

## Acceptance Criteria

- [ ] `new Date()` creates a date object in rquickjs
- [ ] `Date.now()` returns current timestamp
- [ ] `date.toLocaleTimeString(locale, options)` works with common options
- [ ] `date.toISOString()` works
- [ ] `../tui1` example no longer throws `ReferenceError: Date is not defined`
