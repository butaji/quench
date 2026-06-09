# Task 192: `ink-function-expression` Example — Anonymous Function Expressions

**Priority:** P1-High
**Phase:** 17 — Expression-Level TypeScript Features
**Depends on:** 191
**Status:** COMPLETED

## Problem

Anonymous function expressions (`const fn = function(x) { ... }`) and named function expressions (`const fn = function name(x) { ... }`) exercise HIR function handling in expression position. No existing Ink example explicitly targets function expressions.

## Ink Example

```tsx
// examples/ink-function-expression/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// Simple variable
const simpleValue = 42;

// Function expression in IIFE (no params)
const iifeNoParams = (function () {
  return 100;
})();

export default function FunctionExpressionDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Function Expression Demo</Text>
      <Text></Text>
      <Text>Simple value: {simpleValue}</Text>
      <Text>IIFE no params: {iifeNoParams}</Text>
    </Box>
  );
}
```

## Implementation Notes

The example demonstrates:
- Simple variable declarations
- Immediately Invoked Function Expressions (IIFE) with no parameters

For the compile path, IIFE extraction was implemented in:
- `crates/runts-ratatui/src/codegen/vars.rs` - `try_iife_to_rust()`, `extract_return_arg()`
- `crates/runts-ratatui/src/codegen/stmt_collect.rs` - Function expression handling in `extract_call_arg_value_with_type()`

The ratatui codegen now extracts top-level variable declarations (including IIFE) from the HIR module items and generates proper Rust variable declarations.

## Acceptance Criteria

- [x] Example exists at `examples/ink-function-expression/`
- [x] Uses anonymous function expression (IIFE)
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust
- [x] Parity harness passes with 100% match in all 3 environments
