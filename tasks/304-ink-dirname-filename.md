# Task 304: `ink-dirname-filename` Example — `__dirname` and `__filename`

**Priority:** P1-High
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 303

## Problem

`__dirname` and `__filename` are CommonJS globals for file system paths. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-dirname-filename/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const dir = __dirname;
  const file = __filename;

  return (
    <Box flexDirection="column">
      <Text>Dir: {String(dir)}</Text>
      <Text>File: {String(file)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-dirname-filename/`
- [ ] Uses `__dirname` and `__filename`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for CommonJS globals
- [ ] Parity harness passes with 100% match in all 3 environments
