# Task 314: `ink-urlpattern` Example — `URLPattern`

**Priority:** P3-Low
**Phase:** 25 — Web APIs
**Depends on:** 313

## Problem

`URLPattern` provides pattern matching for URLs. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-urlpattern/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const hasPattern = typeof URLPattern !== 'undefined';
  let match = null;

  if (hasPattern) {
    const pattern = new URLPattern({ pathname: '/users/:id' });
    match = pattern.exec('https://example.com/users/42');
  }

  return (
    <Box flexDirection="column">
      <Text>Has URLPattern: {String(hasPattern)}</Text>
      <Text>Match: {match ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-urlpattern/`
- [ ] Uses `URLPattern` constructor and `exec`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
