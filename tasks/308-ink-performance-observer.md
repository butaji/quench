# Task 308: `ink-performance-observer` Example — `PerformanceObserver`, `performance.mark`, `performance.measure`

**Priority:** P2-Medium
**Phase:** 25 — Web APIs
**Depends on:** 307

## Problem

`PerformanceObserver`, `performance.mark()`, and `performance.measure()` provide detailed performance measurement. Task 157 covers `performance.now()` but not the observer API.

## Ink Example

```tsx
// examples/ink-performance-observer/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  performance.mark('start');
  const arr = new Array(1000).fill(0);
  performance.mark('end');
  performance.measure('loop', 'start', 'end');

  const entries = performance.getEntriesByType('measure');
  const duration = entries[0]?.duration ?? 0;

  return (
    <Box flexDirection="column">
      <Text>Duration: {duration.toFixed(2)}ms</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-performance-observer/`
- [ ] Uses `performance.mark`, `measure`, `getEntriesByType`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
