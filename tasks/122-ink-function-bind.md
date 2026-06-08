# Task 122: `ink-function-bind` Example — `bind`, `call`, `apply`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 121
**Status:** completed

## Implementation

This example demonstrates `Function.prototype.bind`, `call`, and `apply` methods:
- `bind` - creates a new function with pre-bound arguments
- `call` - invokes a function with a specified `this` context
- `apply` - invokes a function with arguments as an array

## Files Created/Modified

- `examples/ink-function-bind/tui/app.tsx` - Main component with bind/call/apply usage
- `examples/ink-function-bind/main.tsx` - Entry point
- `examples/ink-function-bind/deno.json` - Deno npm imports
- `examples/ink-function-bind/runts.config.json` - Runts config
- `src/transpile/hir/quote_codegen_calls.inc` - New file for bind/call/apply codegen
- `src/transpile/hir/quote_codegen.rs` - Updated to include calls handler
- `crates/runts-ratatui/src/codegen/vars.rs` - Updated to handle Call expressions

## Acceptance Criteria

- [x] Example exists at `examples/ink-function-bind/`
- [x] Uses `Function.prototype.bind`
- [x] Uses `Function.prototype.call`
- [x] Uses `Function.prototype.apply`
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path builds successfully (expressions not evaluated - ratatui limitation)
- [x] Parity harness shows 100% match between deno and rq (dev path)

## Output Verification

```
 Deno:          Bind: Hey, World!
 Deno:          Call: Hello, World!
 Deno:          Apply: Hi, World!
 Deno:          Partial: 6

 runts dev:     Bind: Hey, World!
 runts dev:     Call: Hello, World!
 runts dev:     Apply: Hi, World!
 runts dev:     Partial: 6

 runts build:   (compiles but expressions not evaluated - ratatui codegen limitation)
```

## Codegen Implementation

The `bind`, `call`, and `apply` methods are handled by:
1. `src/transpile/hir/quote_codegen_calls.inc` - For HIR->Rust codegen
2. `crates/runts-ratatui/src/codegen/vars.rs` - For ratatui plugin JSON->Rust codegen

These transformations:
- `fn.call(null, arg1, arg2)` → `fn(arg1, arg2)` (skip `this` arg)
- `fn.apply(null, [arg1, arg2])` → `fn(arg1, arg2)` (expand array)
- `fn.bind(null, arg1, arg2)` → `move || fn(arg1, arg2)` (create closure)
