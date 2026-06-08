# Task 125: `ink-error-cause` Example — `Error` with `cause`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 124

## Problem

`Error.cause` (ES2022) allows chaining errors to preserve the original error context. It's increasingly common in real-world error handling. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-error-cause/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

function fetchData(): string {
  throw new Error('Network failed');
}

function loadUser(): string {
  try {
    return fetchData();
  } catch (err) {
    throw new Error('Failed to load user', { cause: err });
  }
}

export default function App() {
  const [error, setError] = useState<Error | null>(null);

  try {
    loadUser();
  } catch (err) {
    setError(err as Error);
  }

  return (
    <Box flexDirection="column">
      <Text>Error: {error?.message ?? 'none'}</Text>
      <Text>Cause: {error?.cause ? (error.cause as Error).message : 'none'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-error-cause/`
- [ ] Uses `Error` with `cause` option
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
