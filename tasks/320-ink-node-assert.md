# Task 320: `ink-node-assert` Example — Node.js `assert` Module

**Priority:** P2-Medium
**Phase:** 26 — Node.js Standard Library
**Depends on:** 319

## Problem

The Node.js `assert` module provides runtime assertions (`assert.equal`, `assert.deepStrictEqual`, `assert.throws`, etc.). No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-node-assert/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const a = 1;
  const b = 1;
  const pass = a === b;

  return (
    <Box flexDirection="column">
      <Text>Assert pass: {String(pass)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-node-assert/`
- [ ] Uses assertion patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
