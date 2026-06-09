# Task 289: `ink-variadic-tuple` Example — Variadic Tuple Types

**Priority:** P2-Medium
**Phase:** 24 — Type System Deep Coverage
**Depends on:** 288

## Problem

Variadic tuple types (`type Prefix<T extends unknown[]> = ['start', ...T, 'end']`) model tuples with variable middle sections. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-variadic-tuple/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Wrap<T extends unknown[]> = ['prefix', ...T, 'suffix'];
const wrapped: Wrap<[number, string]> = ['prefix', 42, 'hello', 'suffix'];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Wrapped: {wrapped.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-variadic-tuple/`
- [ ] Uses variadic tuple type with spread
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases variadic tuple types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
