# Task 312: `ink-path-module` Example — `path` Module

**Priority:** P2-Medium
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 311

## Problem

The `path` module provides utilities for file path manipulation. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-path-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const joined = ['a', 'b', 'c'].join('/');
  const base = joined.split('/').pop() ?? '';
  const ext = base.includes('.') ? base.split('.').pop() ?? '' : '';

  return (
    <Box flexDirection="column">
      <Text>Joined: {joined}</Text>
      <Text>Base: {base}</Text>
      <Text>Ext: {ext}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-path-module/`
- [ ] Exercises path manipulation patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
