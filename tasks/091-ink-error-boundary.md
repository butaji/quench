# Task 091: `ink-error-boundary` Example — `<ErrorBoundary>`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Ink's `<ErrorBoundary>` component catches render errors and displays a fallback UI. No existing Ink example exercises error boundaries in a TUI context.

## Ink Example

```tsx
// examples/ink-error-boundary/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text, ErrorBoundary } from 'ink';

function BuggyComponent() {
  const [shouldCrash, setShouldCrash] = useState(false);
  if (shouldCrash) {
    throw new Error('Intentional crash');
  }
  return <Text>Stable</Text>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Main App</Text>
      <ErrorBoundary>
        <BuggyComponent />
      </ErrorBoundary>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-error-boundary/`
- [ ] Uses Ink's `<ErrorBoundary>` component
- [ ] Demonstrates error catching in child component
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles ErrorBoundary
- [ ] Parity harness passes with 100% match in all 3 environments
