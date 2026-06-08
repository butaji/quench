# Task 358: `ink-readonly-mapped` Example — Readonly Mapped Types

**Priority:** P2-Medium
**Phase:** 28 — TypeScript Utility Types
**Depends on:** 357

## Problem

Readonly mapped types (`type ReadonlyDeep<T> = { readonly [K in keyof T]: ReadonlyDeep<T[K]> }`) recursively freeze object shapes. No existing Ink example exercises recursive readonly mapped types.

## Ink Example

```tsx
// examples/ink-readonly-mapped/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type ReadonlyDeep<T> = {
  readonly [K in keyof T]: T[K] extends object ? ReadonlyDeep<T[K]> : T[K];
};

interface Config {
  server: { host: string; port: number };
}

const config: ReadonlyDeep<Config> = {
  server: { host: 'localhost', port: 3000 },
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Host: {config.server.host}</Text>
      <Text>Port: {config.server.port}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-readonly-mapped/`
- [ ] Uses recursive readonly mapped type
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases readonly without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
