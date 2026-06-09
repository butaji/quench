# Task 278: `ink-escape-unescape` Example — Legacy `escape()` and `unescape()`

**Priority:** P3-Low
**Phase:** 23 — Legacy Language Features
**Depends on:** 277

## Problem

`escape()` and `unescape()` are deprecated legacy functions. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-escape-unescape/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const input = 'hello world';
  const escaped = escape(input);
  const unescaped = unescape(escaped);

  return (
    <Box flexDirection="column">
      <Text>Original: {input}</Text>
      <Text>Escaped: {escaped}</Text>
      <Text>Unescaped: {unescaped}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-escape-unescape/`
- [ ] Uses `escape()` and `unescape()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
