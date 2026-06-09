# Task 423: `ink-performance-navigation` Example — `performance.timing`, `performance.navigation`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 422

## Problem

Legacy `performance.timing` and `performance.navigation` APIs (deprecated but still present) are not explicitly exercised. Tasks 157 and 308 cover `performance.now`, `mark`, `measure`, and `PerformanceObserver`, but the legacy timing APIs are missing.

## HIR Coverage

- `Expr::Member` for `performance.timing` and `performance.navigation`
- `Expr::Member` for nested properties like `performance.timing.navigationStart`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-performance-navigation/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const timing = performance.timing;
const navStart = timing?.navigationStart ?? 0;
const domComplete = timing?.domComplete ?? 0;
const navType = performance.navigation?.type ?? 0;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>NavStart: {navStart}</Text>
      <Text>DomComplete: {domComplete}</Text>
      <Text>NavType: {navType}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-performance-navigation/`
- [ ] Uses `performance.timing` and `performance.navigation`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
