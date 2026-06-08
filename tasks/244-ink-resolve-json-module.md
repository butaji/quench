# Task 244: `ink-resolve-json-module` Example — `resolveJsonModule`

**Priority:** P1-High
**Phase:** 21 — TypeScript Configuration
**Depends on:** 243

## Problem

`resolveJsonModule` allows importing `.json` files as typed modules. Task 180 covers import attributes for JSON, but `resolveJsonModule` is the traditional TypeScript option. No dedicated example exists.

## Ink Example

```tsx
// examples/ink-resolve-json-module/config.json
{ "name": "App", "version": "1.0.0" }

// examples/ink-resolve-json-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import config from '../config.json';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {config.name}</Text>
      <Text>Version: {config.version}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-resolve-json-module/`
- [ ] Includes `tsconfig.json` with `resolveJsonModule: true`
- [ ] Imports a `.json` file
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles JSON module imports
- [ ] Parity harness passes with 100% match in all 3 environments
