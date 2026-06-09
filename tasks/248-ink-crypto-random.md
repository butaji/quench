# Task 248: `ink-crypto-random` Example — `crypto.randomUUID`

**Priority:** P2-Medium
**Phase:** 21 — Runtime API Deep Coverage
**Depends on:** 247

## Problem

`crypto.randomUUID()` generates RFC 4122 version 4 UUIDs. No existing Ink example exercises the Web Crypto API.

## Ink Example

```tsx
// examples/ink-crypto-random/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const uuid = crypto.randomUUID();
  const array = new Uint8Array(4);
  crypto.getRandomValues(array);

  return (
    <Box flexDirection="column">
      <Text>UUID: {uuid}</Text>
      <Text>Random: {Array.from(array).join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-crypto-random/`
- [ ] Uses `crypto.randomUUID()` and `crypto.getRandomValues()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Web Crypto API
- [ ] Parity harness passes with 100% match in all 3 environments
