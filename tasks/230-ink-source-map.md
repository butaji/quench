# Task 230: `ink-source-map` Example — Source Map Generation for Compiled Output

**Priority:** P2-Medium
**Phase:** 20 — Compile Path Infrastructure
**Depends on:** 229

## Problem

Source maps map compiled Rust output back to original TS/TSX source for debugging. No existing task covers source map generation.

## Ink Example

```tsx
// examples/ink-source-map/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const message = 'Source maps work!';

  return (
    <Box flexDirection="column">
      <Text>{message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-source-map/`
- [ ] Compile path generates `.map` file alongside compiled binary
- [ ] Source map maps Rust lines back to TS/TSX source
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Parity harness passes with 100% match in all 3 environments
