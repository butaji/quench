# Task 271: `ink-temporal-api` Example — Temporal API

**Priority:** P3-Low
**Phase:** 23 — Emerging / Stage 3 Features
**Depends on:** 270

## Problem

The Temporal API is a modern date/time replacement for `Date`. It is currently Stage 3 and available behind flags in some runtimes. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-temporal-api/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const instant = Temporal.Now.instant();
  const date = instant.toLocaleString('en-US');
  const epoch = instant.epochMilliseconds;

  return (
    <Box flexDirection="column">
      <Text>Date: {date}</Text>
      <Text>Epoch: {epoch}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-temporal-api/`
- [ ] Uses `Temporal` API
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `Temporal` global or documents gap
- [ ] Parity harness passes with 100% match in all 3 environments
