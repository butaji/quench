# Task 085: `ink-import-export-type` Example — `import type`, `export type`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`import type` and `export type` are essential TypeScript patterns for type-only imports/exports that are erased at compile time. No existing Ink example exercises these constructs.

## Ink Example

```tsx
// examples/ink-import-export-type/types.ts
export type Status = 'idle' | 'loading' | 'success' | 'error';
export type Theme = 'light' | 'dark';
export interface Config {
  status: Status;
  theme: Theme;
}

// examples/ink-import-export-type/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import type { Status, Theme } from '../types.js';
import { Config } from '../types.js'; // value import if Config is a class

const cfg: Config = { status: 'success', theme: 'dark' };

export type { Status };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Status: {cfg.status}</Text>
      <Text>Theme: {cfg.theme}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-export-type/`
- [ ] Uses `import type` for type-only imports
- [ ] Uses `export type` for type-only re-exports
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases all `import type`/`export type` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments