# Task 292: `ink-type-only-import-assertion` Example — `import type` with Import Attributes

**Priority:** P2-Medium
**Phase:** 24 — Module Patterns
**Depends on:** 291

## Problem

Combining `import type` with import attributes (`import type Config from './config.json' with { type: 'json' }`) is an edge case in module parsing. No existing example exercises this combination.

## Ink Example

```tsx
// examples/ink-type-only-import-assertion/config.json
{ "name": "App" }

// examples/ink-type-only-import-assertion/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Config {
  name: string;
}

const config: Config = { name: 'App' };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {config.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-type-only-import-assertion/`
- [ ] Documents `import type` with attributes pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles type-only imports with attributes
- [ ] Parity harness passes with 100% match in all 3 environments
