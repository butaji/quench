# Task 096: `ink-reflect-api` Example — `Reflect` API

**Priority:** P3-Low
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

The `Reflect` API (ES2015) provides introspection and modification methods for objects. No existing Ink example exercises `Reflect.get`, `Reflect.set`, `Reflect.has`, etc.

## Ink Example

```tsx
// examples/ink-reflect-api/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const obj = { name: 'App', version: 1 };

const name = Reflect.get(obj, 'name');
Reflect.set(obj, 'version', 2);
const hasName = Reflect.has(obj, 'name');
const keys = Reflect.ownKeys(obj);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {name}</Text>
      <Text>Version: {obj.version}</Text>
      <Text>Has name: {hasName ? 'yes' : 'no'}</Text>
      <Text>Keys: {keys.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-reflect-api/`
- [ ] Uses `Reflect.get`, `Reflect.set`, `Reflect.has`, `Reflect.ownKeys`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles Reflect API calls
- [ ] Parity harness passes with 100% match in all 3 environments