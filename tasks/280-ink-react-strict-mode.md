# Task 280: `ink-react-strict-mode` Example — `React.StrictMode`

**Priority:** P2-Medium
**Phase:** 23 — React Patterns
**Depends on:** 279

## Problem

`React.StrictMode` activates additional checks and warnings. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-react-strict-mode/tui/app.tsx
import React, { StrictMode } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <StrictMode>
      <Box flexDirection="column">
        <Text>StrictMode example</Text>
      </Box>
    </StrictMode>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-strict-mode/`
- [ ] Uses `React.StrictMode` wrapper
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles StrictMode JSX element
- [ ] Parity harness passes with 100% match in all 3 environments
