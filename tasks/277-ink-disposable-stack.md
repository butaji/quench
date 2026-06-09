# Task 277: `ink-disposable-stack` Example — `DisposableStack` and `AsyncDisposableStack`

**Priority:** P3-Low
**Phase:** 23 — ES2024/ES2025 Features
**Depends on:** 276

## Problem

`DisposableStack` and `AsyncDisposableStack` (Stage 3/ES2024+) manage multiple disposable resources. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-disposable-stack/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const stack = new DisposableStack();
  stack.defer(() => {
    // cleanup
  });

  const disposed = stack.disposed;
  stack.dispose();

  return (
    <Box flexDirection="column">
      <Text>Disposed: {String(disposed)}</Text>
      <Text>After dispose: {String(stack.disposed)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-disposable-stack/`
- [ ] Uses `DisposableStack` constructor and methods
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
