# Task 247: `ink-text-decoder` Example — `TextDecoder`

**Priority:** P2-Medium
**Phase:** 21 — Runtime API Deep Coverage
**Depends:** 246

## Problem

`TextDecoder` converts byte arrays to strings. Task 218 covers `TextEncoder` but not `TextDecoder`.

## Ink Example

```tsx
// examples/ink-text-decoder/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const encoder = new TextEncoder();
  const encoded = encoder.encode('Hello, World!');
  const decoder = new TextDecoder('utf-8');
  const decoded = decoder.decode(encoded);

  return (
    <Box flexDirection="column">
      <Text>Original: Hello, World!</Text>
      <Text>Decoded: {decoded}</Text>
      <Text>Bytes: {encoded.length}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-text-decoder/`
- [ ] Uses `TextDecoder` with `decode()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for TextDecoder
- [ ] Parity harness passes with 100% match in all 3 environments
