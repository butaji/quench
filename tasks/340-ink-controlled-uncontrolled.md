# Task 340: `ink-controlled-uncontrolled` Example — Controlled vs Uncontrolled Components

**Priority:** P1-High
**Phase:** 27 — React Patterns
**Depends on:** 339

## Problem

Controlled components (state-driven value) vs uncontrolled components (internal state / refs) are fundamental React patterns. No dedicated Ink example exercises both.

## Ink Example

```tsx
// examples/ink-controlled-uncontrolled/tui/app.tsx
import React, { useState, useRef } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [controlled, setControlled] = useState('controlled');
  const uncontrolledRef = useRef('uncontrolled');

  return (
    <Box flexDirection="column">
      <Text>Controlled: {controlled}</Text>
      <Text>Uncontrolled: {uncontrolledRef.current}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-controlled-uncontrolled/`
- [ ] Demonstrates controlled and uncontrolled patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
