# Task 415: `ink-atob-btoa` Example — `atob`, `btoa`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 414

## Problem

Base64 encoding/decoding via `atob` and `btoa` are standard Web APIs available in Deno and Node.js. No existing Ink example exercises these functions.

## HIR Coverage

- `Expr::Call` for global `atob()` and `btoa()` functions
- `Expr::Ident` for global identifiers

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-atob-btoa/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const original = 'Hello, World!';
const encoded = btoa(original);
const decoded = atob(encoded);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Original: {original}</Text>
      <Text>Encoded: {encoded}</Text>
      <Text>Decoded: {decoded}</Text>
      <Text>Match: {original === decoded ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-atob-btoa/`
- [ ] Uses `atob` and `btoa`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
