# Task 217: `ink-module-resolution` Example — `moduleResolution` Modes (`node`, `bundler`, `classic`)

**Priority:** P1-High
**Phase:** 19 — TypeScript Configuration
**Depends on:** 216

## Problem

TypeScript `moduleResolution` modes (`node`, `node16`, `nodenext`, `bundler`, `classic`) control how module specifiers are resolved. No existing Ink example exercises different resolution modes.

## Ink Example

```tsx
// examples/ink-module-resolution/tsconfig.json
{
  "compilerOptions": {
    "moduleResolution": "bundler"
  }
}

// examples/ink-module-resolution/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Module resolution: bundler</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-module-resolution/`
- [ ] Includes `tsconfig.json` with `moduleResolution: "bundler"`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `moduleResolution` setting
- [ ] Parity harness passes with 100% match in all 3 environments
