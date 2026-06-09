# Task 327: `ink-file-constructor` Example — `File` Constructor

**Priority:** P2-Medium
**Phase:** 26 — Web APIs
**Depends on:** 326

## Problem

The `File` constructor extends `Blob` with name and last-modified properties. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-file-constructor/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const file = new File(['content'], 'test.txt', { type: 'text/plain' });
  const info = { name: file.name, size: file.size, type: file.type };

  return (
    <Box flexDirection="column">
      <Text>Has File: {String(hasFile)}</Text>
      <Text>Name: {info.name}</Text>
      <Text>Size: {info.size}</Text>
      <Text>Type: {info.type}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `ClassMember` and `Class` variants
- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-file-constructor/`
- [ ] Uses `File` constructor or documents availability
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
