# Task 142: `ink-inline-type-import` Example — `import { type X }`, `import type * as ns`

**Priority:** P1-High
**Phase:** 12 — Module Pattern Completion
**Depends on:** 085

## Problem

Inline type imports (`import { type X }` from TS 4.5) and namespace type imports (`import type * as ns`) are modern TypeScript patterns for explicit type-only imports. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-inline-type-import/types.ts
export interface User {
  name: string;
  age: number;
}
export type Status = 'active' | 'inactive';

// examples/ink-inline-type-import/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import { type User, type Status } from '../types.js';
import type * as Types from '../types.js';

const user: User = { name: 'Alice', age: 30 };
const status: Status = 'active';
const altUser: Types.User = { name: 'Bob', age: 25 };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {user.name}</Text>
      <Text>Status: {status}</Text>
      <Text>Alt: {altUser.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-inline-type-import/`
- [ ] Uses `import { type X }` inline type import
- [ ] Uses `import type * as ns` namespace type import
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases all type imports without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
