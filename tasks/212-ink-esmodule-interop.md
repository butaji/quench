# Task 212: `ink-esmodule-interop` Example — `esModuleInterop` and `allowSyntheticDefaultImports`

**Priority:** P1-High
**Phase:** 19 — TypeScript Configuration
**Depends on:** 211

## Problem

`esModuleInterop` and `allowSyntheticDefaultImports` affect how CommonJS modules are imported in TypeScript. No existing Ink example exercises these compiler options.

## Ink Example

```tsx
// examples/ink-esmodule-interop/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// With esModuleInterop, this import style works for CommonJS modules
import * as path from 'path';

export default function App() {
  const joined = path.join('a', 'b', 'c');

  return (
    <Box flexDirection="column">
      <Text>Joined: {joined}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-esmodule-interop/`
- [ ] Uses namespace import from CommonJS-style module
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles synthetic default imports
- [ ] Parity harness passes with 100% match in all 3 environments
