# Task 318: `ink-node-stream` Example — Node.js `stream` Module

**Priority:** P1-High
**Phase:** 26 — Node.js Standard Library
**Depends on:** 317

## Problem

The Node.js `stream` module provides `Readable`, `Writable`, `Transform`, `Duplex`, and `Pipeline`. No existing Ink example exercises Node.js streams.

## Ink Example

```tsx
// examples/ink-node-stream/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const chunks: string[] = [];
  chunks.push('a');
  chunks.push('b');
  chunks.push('c');

  return (
    <Box flexDirection="column">
      <Text>Stream chunks: {chunks.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-node-stream/`
- [ ] References Node.js stream patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for stream constructors
- [ ] Parity harness passes with 100% match in all 3 environments
