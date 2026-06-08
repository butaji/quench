# Task 135: Add `Date` Object to rquickjs Bridge

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 132
**Status:** Completed

## Problem

The `../tui1` example creates `Date` instances repeatedly:

```tsx
const timestamp = new Date().toLocaleTimeString("en-US", {
  hour: "2-digit", minute: "2-digit", hour12: false
});
```

`Date` may or may not exist in rquickjs. If it does not, the example crashes with `ReferenceError: Date is not defined`.

## Solution

rquickjs has a **built-in Date** implementation that is fully compatible with JavaScript's standard Date API. We do NOT need to create a custom Date polyfill - doing so would actually break things by overriding the working built-in.

## Acceptance Criteria

- [x] `new Date()` creates a date object in rquickjs
- [x] `Date.now()` returns current timestamp
- [x] `date.toLocaleTimeString(locale, options)` works with common options
- [x] `date.toISOString()` works
- [x] `ink-date-math` example passes rquickjs parity test

## Notes

- Do NOT install a custom Date implementation - it will override rquickjs's built-in Date
- The built-in Date supports all standard methods: getFullYear, getMonth, getDate, getHours, etc.
- The ink-date-math test passes with rquickjs's built-in Date
