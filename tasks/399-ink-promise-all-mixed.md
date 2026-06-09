# Task 399: `ink-promise-all-mixed` Example — Promise.all with Mixed Resolve/Reject

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 398

## Problem

`Promise.all` with a mix of resolved and rejected promises exercises the rejection handling of Promise combinators. No existing Ink example explicitly tests this edge case.

## HIR Coverage

- `Expr::Call` for `Promise.all` with array argument.
- `.catch()` chaining on Promise.all result.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for Promise.all with mixed outcomes.
- `.catch()` handler must be emitted for rejection handling.

## Ink Example

```tsx
// examples/ink-promise-all-mixed/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [result, setResult] = useState('pending');

  useEffect(() => {
    Promise.all([
      Promise.resolve('ok1'),
      Promise.reject(new Error('fail')),
      Promise.resolve('ok2')
    ]).then((values) => {
      setResult(`all: ${values.join(', ')}`);
    }).catch((err) => {
      setResult(`catch: ${err.message}`);
    });
  }, []);

  return (
    <Box>
      <Text>{result}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-promise-all-mixed/`
- [ ] Uses `Promise.all` with mixed resolve/reject
- [ ] `.catch()` handler executes on rejection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
