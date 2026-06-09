# Task 253: `ink-blob` Example — Blob and FileReader APIs

**Priority:** P2-Medium
**Phase:** 22 — Web APIs + Event System
**Depends on:** 252

## Problem

`Blob` and `FileReader` provide binary data handling in JavaScript. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-blob/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [text, setText] = useState('');

  useEffect(() => {
    const blob = new Blob(['hello world']);
    const reader = new FileReader();
    reader.onload = () => setText(String(reader.result));
    reader.readAsText(blob);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Blob text: {text}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-blob/`
- [ ] Uses `Blob` constructor and `FileReader`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
