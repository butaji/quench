# Task 276: `ink-iterator-from` Example — `Iterator.from` and Iterator Helpers

**Priority:** P3-Low
**Phase:** 23 — ES2025 Features
**Depends on:** 275

## Problem

`Iterator.from` converts iterables into iterator objects. It is part of the ES2025 iterator helpers proposal. Task 190 covers helper methods but not `Iterator.from`.

## Ink Example

```tsx
// examples/ink-iterator-from/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const iterable = [1, 2, 3, 4, 5];
  const iter = Iterator.from(iterable);
  const mapped = iter.map((x: number) => x * 2);
  const result: number[] = [];

  for (let i = 0; i < 3; i++) {
    const next = mapped.next();
    if (next.done) break;
    result.push(next.value);
  }

  return (
    <Box flexDirection="column">
      <Text>First 3: {result.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-iterator-from/`
- [ ] Uses `Iterator.from`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
