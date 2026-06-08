# Task 187: `ink-throw-expression` Example — Throw Expressions (Stage 3)

**Priority:** P2-Medium
**Phase:** 17 — Stage 3 TC39 Features
**Depends on:** 186

## Problem

Throw expressions (`const x = y ?? throw new Error()`) are a Stage 3 TC39 proposal that allows `throw` in expression positions. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-throw-expression/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function assertDefined<T>(value: T | undefined, msg: string): T {
  return value ?? (() => { throw new Error(msg); })();
}

const name = assertDefined('World', 'Name required');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Hello {name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-throw-expression/`
- [ ] Uses throw expression or IIFE equivalent
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
