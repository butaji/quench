# Task 317: `ink-node-util` Example — Node.js `util` Module

**Priority:** P1-High
**Phase:** 26 — Node.js Standard Library
**Depends on:** 316

## Problem

The Node.js `util` module provides `promisify`, `inspect`, `types` (isPromise, isArray, etc.), `format`, and `inherits`. No existing Ink example exercises these utilities.

## Ink Example

```tsx
// examples/ink-node-util/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: 1, b: [2, 3] };
  const formatted = JSON.stringify(obj, null, 2);

  return (
    <Box flexDirection="column">
      <Text>Inspect: {formatted}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-node-util/`
- [ ] Uses `util`-like patterns (promisify, inspect, format)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
