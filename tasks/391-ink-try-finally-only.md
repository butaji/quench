# Task 391: `ink-try-finally-only` Example — `try...finally` without `catch`

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 390

## Problem

JavaScript supports `try...finally` blocks without a `catch` clause. The `finally` block always executes, even if the `try` block throws. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Stmt::TryCatch` with `handler: None` (no catch block).
- `finalizer` field must be present and always executed.

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust for `try...finally` without catch.
- Generated code must ensure `finally` block runs on both normal and exceptional paths.

## Ink Example

```tsx
// examples/ink-try-finally-only/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

function runWithFinally(): string {
  let log = 'start';
  try {
    log += ', try';
    return log;
  } finally {
    log += ', finally';
  }
}

export default function App() {
  const result = runWithFinally();
  return (
    <Box flexDirection="column">
      <Text>Result: {result}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-try-finally-only/`
- [ ] Uses `try...finally` without `catch`
- [ ] HIR `TryCatch` with `handler: None` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
