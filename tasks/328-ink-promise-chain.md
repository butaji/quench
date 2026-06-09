# Task 328: `ink-promise-chain` Example — `Promise.prototype.then/catch/finally` + `Promise.any`/`race`

**Priority:** P1-High
**Phase:** 26 — Promise API Completion
**Depends on:** 327

## Problem

Promise chaining methods (`then`, `catch`, `finally`) and static methods (`any`, `race`) are core async primitives. Tasks 114/176/182 cover some Promise statics but no dedicated example exercises `then/catch/finally` chaining or `Promise.any`/`race`.

## Ink Example

```tsx
// examples/ink-promise-chain/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [result, setResult] = useState('');

  useEffect(() => {
    Promise.resolve(1)
      .then(v => v + 1)
      .then(v => v * 2)
      .catch(() => 0)
      .finally(() => setResult('done'));
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Result: {result}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-promise-chain/`
- [ ] Uses `then`, `catch`, `finally` chaining
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
