# Task 422: `ink-request-animation-frame` Example — `requestAnimationFrame`, `cancelAnimationFrame`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 421

## Problem

`requestAnimationFrame` and `cancelAnimationFrame` are standard Web APIs for animation timing. No existing Ink example exercises these functions.

## HIR Coverage

- `Expr::Call` for global `requestAnimationFrame()` and `cancelAnimationFrame()`
- `Expr::Ident` for global identifiers
- Arrow function callbacks passed to `requestAnimationFrame`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-request-animation-frame/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [count, setCount] = useState(0);

  useEffect(() => {
    let id: number;
    const tick = () => {
      setCount((c) => c + 1);
      if (count < 3) {
        id = requestAnimationFrame(tick);
      }
    };
    id = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(id);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Frame count: {count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-request-animation-frame/`
- [ ] Uses `requestAnimationFrame` and `cancelAnimationFrame`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
