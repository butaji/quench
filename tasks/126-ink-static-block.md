# Task 126: `ink-static-block` Example — Class `static {}` Blocks

**Priority:** P1-High
**Phase:** 12 — ES2022+ Language Features
**Depends on:** 125

## Problem

Class `static {}` blocks (ES2022) allow arbitrary initialization logic for class static members. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-static-block/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Config {
  static env: string;
  static version: string;

  static {
    this.env = 'production';
    this.version = '1.0.0';
  }

  static {
    if (this.env === 'development') {
      this.version += '-dev';
    }
  }
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Env: {Config.env}</Text>
      <Text>Version: {Config.version}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-static-block/`
- [ ] Uses class `static {}` block
- [ ] Uses multiple static blocks
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
