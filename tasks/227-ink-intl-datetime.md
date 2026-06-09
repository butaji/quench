# Task 227: `ink-intl-datetime` Example — `Intl.DateTimeFormat`

**Priority:** P2-Medium
**Phase:** 20 — Runtime API Deep Coverage
**Depends on:** 226

## Problem

`Intl.DateTimeFormat` formats dates and times for specific locales. No existing Ink example exercises `Intl` APIs.

## Ink Example

```tsx
// examples/ink-intl-datetime/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const date = new Date('2024-01-15T10:30:00');
  const formatter = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });
  const formatted = formatter.format(date);

  return (
    <Box flexDirection="column">
      <Text>Date: {formatted}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-intl-datetime/`
- [ ] Uses `Intl.DateTimeFormat` with options
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Intl APIs
- [ ] Parity harness passes with 100% match in all 3 environments
