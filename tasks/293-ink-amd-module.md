# Task 293: `ink-amd-module` Example — `/// <reference amd-module name="..." />

**Priority:** P3-Low
**Phase:** 24 — Legacy Module Systems
**Depends on:** 292

## Problem

Triple-slash `amd-module` directives control AMD module names. No existing Ink example exercises this legacy directive.

## Ink Example

```tsx
/// <reference amd-module name="app"/>

// examples/ink-amd-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>AMD module directive example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-amd-module/`
- [ ] Uses `/// <reference amd-module name="..." />` directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips AMD directive without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
