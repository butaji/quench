# Task 120: `ink-global-augmentation` Example — `global` Augmentation, Module Augmentation

**Priority:** P3-Low
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

Global augmentation (`declare global`) and module augmentation (`declare module`) are TypeScript patterns for extending existing types. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-global-augmentation/types.d.ts
declare global {
  interface Window {
    appVersion: string;
  }
  var __BUILD_TIME__: string;
}

declare module 'ink' {
  interface BoxProps {
    'data-testid'?: string;
  }
}

export {};

// examples/ink-global-augmentation/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const buildTime = typeof __BUILD_TIME__ !== 'undefined' ? __BUILD_TIME__ : 'dev';

export default function App() {
  return (
    <Box flexDirection="column" data-testid="root">
      <Text>Build: {buildTime}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-global-augmentation/`
- [ ] Uses `declare global` to augment global interface
- [ ] Uses `declare module` to augment external module
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases augmentations without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
