# Task 009: Rust: Expose ink module to JS

## Goal
Expose all ink functionality from Rust to JS via rquickjs globals.

## Status
> ✅ **Done** - Code exists in `src/ink_js.rs`
> ⚠️ **Not Integrated** - See Task 009b for integration blocker

## Acceptance Criteria
- [x] `ink::BOX`, `ink::TEXT`, etc. exposed as JS string constants
- [x] `ink::render()` exposed as JS function
- [x] `ink::use_state()`, `ink::use_effect()`, etc. exposed as JS functions
- [x] Hook context managed in Rust, not JS
- [x] JS receives data structures only, all logic in Rust

## Code Location
- `src/ink_js.rs` - Full implementation
- `ink_js::register()` - Called to expose globals

## Dependencies
- Task 009b (integration)

## SPEC Reference
§3 Rust Modules
