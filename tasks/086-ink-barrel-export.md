# Task 086: `ink-barrel-export` Example — Barrel Files, `export * from`, `import * as`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Barrel files (`export * from './module'`) and namespace imports (`import * as ns`) are standard patterns for organizing module boundaries. No existing Ink example exercises these.

## Ink Example

```tsx
// examples/ink-barrel-export/components/index.ts
export * from './Header.js';
export * from './Footer.js';
export { default as Body } from './Body.js';

// examples/ink-barrel-export/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import * as Components from '../components/index.js';

export default function App() {
  return (
    <Box flexDirection="column">
      <Components.Header />
      <Components.Body />
      <Components.Footer />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-barrel-export/`
- [ ] Uses `export * from` barrel pattern
- [ ] Uses `import * as ns` namespace import
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles namespace imports and barrel re-exports
- [ ] Parity harness passes with 100% match in all 3 environments