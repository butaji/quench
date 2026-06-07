# Task 090: `ink-suspense-lazy` Example — `Suspense`, `lazy`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`React.Suspense` and `React.lazy` are standard patterns for code splitting and async component loading. No existing Ink example exercises these React APIs.

## Ink Example

```tsx
// examples/ink-suspense-lazy/tui/app.tsx
import React, { Suspense, lazy } from 'react';
import { Box, Text } from 'ink';

const LazyComponent = lazy(() => import('./LazyPanel.js'));

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Header</Text>
      <Suspense fallback={<Text>Loading...</Text>}>
        <LazyComponent />
      </Suspense>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-suspense-lazy/`
- [ ] Uses `React.lazy` for dynamic component import
- [ ] Uses `React.Suspense` with fallback
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `lazy` and `Suspense`
- [ ] Parity harness passes with 100% match in all 3 environments
