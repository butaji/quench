# Task 273: `ink-suppressed-error` Example — `SuppressedError` and `Error.isError`

**Priority:** P3-Low
**Phase:** 23 — Runtime API Completion
**Depends on:** 272

## Problem

`SuppressedError` (ES2025) wraps an error suppressed by another error. `Error.isError` (proposal) checks if a value is an error. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-suppressed-error/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const err = new Error('primary');
  const suppressed = new Error('suppressed');

  const hasIsError = 'isError' in Error;
  const check = hasIsError ? (Error as any).isError(err) : err instanceof Error;

  return (
    <Box flexDirection="column">
      <Text>isError: {String(check)}</Text>
      <Text>Message: {err.message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-suppressed-error/`
- [ ] Uses `Error.isError` or polyfill
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
