# Task 303: `ink-buffer` Example — Node.js `Buffer`

**Priority:** P1-High
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 302

## Problem

`Buffer` is the Node.js built-in for handling binary data. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-buffer/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const buf = Buffer.from('hello', 'utf-8');
  const hex = buf.toString('hex');
  const base64 = buf.toString('base64');
  const length = buf.length;

  return (
    <Box flexDirection="column">
      <Text>Hex: {hex}</Text>
      <Text>Base64: {base64}</Text>
      <Text>Length: {length}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-buffer/`
- [ ] Uses `Buffer.from` and `toString`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Buffer
- [ ] Parity harness passes with 100% match in all 3 environments
