# Task 388: `ink-switch-fallthrough` Example — `switch` with Intentional Fallthrough

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 387

## Problem

`switch` statements in JavaScript can have intentional fallthrough between cases (when `break` is omitted). This exercises the HIR `Switch` statement's handling of case blocks without explicit `break`.

## HIR Coverage

- `Stmt::Switch` with multiple cases sharing the same block body.
- `Stmt::Break` is optional in fallthrough cases.

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust for `switch` with fallthrough.
- Fallthrough cases map to sequential `if` branches without early returns.

## Ink Example

```tsx
// examples/ink-switch-fallthrough/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function getDayType(day: string): string {
  switch (day) {
    case 'monday':
    case 'tuesday':
    case 'wednesday':
    case 'thursday':
    case 'friday':
      return 'weekday';
    case 'saturday':
    case 'sunday':
      return 'weekend';
    default:
      return 'unknown';
  }
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Mon: {getDayType('monday')}</Text>
      <Text>Sat: {getDayType('saturday')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-switch-fallthrough/`
- [ ] Uses `switch` with intentional fallthrough cases
- [ ] HIR `Switch` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
