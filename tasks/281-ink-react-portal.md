# Task 281: `ink-react-portal` Example — `ReactDOM.createPortal`

**Priority:** P3-Low
**Phase:** 23 — React Patterns
**Depends on:** 280

## Problem

`createPortal` renders children into a DOM node outside the parent hierarchy. Ink is terminal-based so true portals are not applicable, but the API may appear in shared component code.

## Ink Example

```tsx
// examples/ink-react-portal/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>createPortal is not applicable to Ink terminal renderer.</Text>
      <Text>This example documents the gap.</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-portal/`
- [ ] Documents `createPortal` gap for terminal rendering
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path either supports portal or produces clear error
- [ ] Parity harness passes with 100% match in all 3 environments
