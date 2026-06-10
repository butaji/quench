# Task 010: Rust: render() Function

## Goal
Implement `render()` in Rust and expose it to JS via rquickjs.

## Status
> ✅ **Done** - Code exists in `src/ink_js.rs::render()`
> ⚠️ **Not Integrated** - See Task 009b

## Acceptance Criteria
- [x] `render(element)` accepts JS element object `{type, props}`
- [x] Builds Rust node tree from JS element
- [x] Calculates Yoga layout
- [x] Returns JS object with `{waitUntilExit, unmount, rerender}`
- [x] No logic in JS

## Code Location
- `src/ink_js.rs::render()` - Full implementation

## Dependencies
- Task 009b (integration)

## SPEC Reference
§3 Rust Modules; §4 JS Runtime
