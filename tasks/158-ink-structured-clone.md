# Task 158: `ink-structured-clone` Example — `structuredClone`

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 157

## Problem

`structuredClone()` (ES2022) is the standard way to deep-clone JavaScript values including objects, arrays, maps, sets, and ArrayBuffers. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-structured-clone/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const original = {
  name: 'App',
  config: { theme: 'dark', lang: 'en' },
  tags: ['ui', 'api'],
};

const cloned = structuredClone(original);
cloned.config.theme = 'light';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Original theme: {original.config.theme}</Text>
      <Text>Cloned theme: {cloned.config.theme}</Text>
      <Text>Same obj: {original === cloned ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-structured-clone/`
- [ ] Uses `structuredClone()` for deep copy
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
