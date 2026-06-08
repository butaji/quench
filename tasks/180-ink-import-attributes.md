# Task 180: `ink-import-attributes` Example — `import` with Attributes (`with { type: "json" }`)

**Priority:** P1-High
**Phase:** 17 — ES2024 / TS 5.3 Features
**Depends on:** 179

## Problem

Import attributes (`import mod from "./data.json" with { type: "json" }`) are an ES2024 feature for specifying metadata about imported modules. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-import-attributes/data.json
{ "name": "App", "version": "1.0.0" }

// examples/ink-import-attributes/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import data from './data.json' with { type: 'json' };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {data.name}</Text>
      <Text>Version: {data.version}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-attributes/`
- [ ] Uses `import ... with { type: 'json' }` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles import attributes
- [ ] Parity harness passes with 100% match in all 3 environments
