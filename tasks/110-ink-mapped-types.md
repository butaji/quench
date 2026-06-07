# Task 110: `ink-mapped-types` Example — Mapped Types, `keyof`, `in`

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

Mapped types (`{ [K in keyof T]: V }`) are a core TypeScript metaprogramming feature for transforming types. No existing Ink example explicitly exercises them in a TUI context.

## Ink Example

```tsx
// examples/ink-mapped-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface User {
  name: string;
  age: number;
  active: boolean;
}

type NullableUser = { [K in keyof User]: User[K] | null };
type UserStrings = { [K in keyof User]: string };
type OptionalUser = { [K in keyof User]?: User[K] };

const partial: OptionalUser = { name: 'Alice' };

function keysOf<T extends object>(obj: T): (keyof T)[] {
  return Object.keys(obj) as (keyof T)[];
}

const userKeys = keysOf({ name: 'Bob', age: 30 });

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {partial.name ?? 'N/A'}</Text>
      <Text>Keys: {userKeys.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-mapped-types/`
- [ ] Uses mapped type `[K in keyof T]: V`
- [ ] Uses `keyof` operator with generic function
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases mapped types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
