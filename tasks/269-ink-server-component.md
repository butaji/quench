# Task 269: `ink-server-component` Example — `"use server"` / `"use client"` Directives

**Priority:** P3-Low
**Phase:** 22 — React Patterns
**Depends on:** 268

## Problem

React Server Components use `"use server"` and `"use client"` directives to distinguish server and client boundaries. No existing Ink example exercises these directives.

## Ink Example

```tsx
// examples/ink-server-component/tui/app.tsx
'use client';

import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Client component directive example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-server-component/`
- [ ] Uses `"use client"` directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips or preserves directive as needed
- [ ] Parity harness passes with 100% match in all 3 environments
