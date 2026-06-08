# Task 302: `ink-global-object` Example — `global` Object

**Priority:** P1-High
**Phase:** 25 — Runtime Globals
**Depends on:** 301

## Problem

`global` is the Node.js-specific global object (before `globalThis`). No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-global-object/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const g = typeof global !== 'undefined' ? global : globalThis;
  const hasConsole = 'console' in g;

  return (
    <Box flexDirection="column">
      <Text>Has console: {String(hasConsole)}</Text>
      <Text>Global type: {typeof g}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-global-object/`
- [ ] Uses `global` or `globalThis` fallback
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `global`
- [ ] Parity harness passes with 100% match in all 3 environments
