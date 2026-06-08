# Task 291: `ink-branded-types` Example — Branded / Opaque Types

**Priority:** P2-Medium
**Phase:** 24 — Type System Patterns
**Depends on:** 290

## Problem

Branded types (`type UserId = string & { __brand: 'UserId' }`) create nominal-like typing on top of structural types. No existing Ink example explicitly exercises branded types.

## Ink Example

```tsx
// examples/ink-branded-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type UserId = string & { __brand: 'UserId' };
type PostId = string & { __brand: 'PostId' };

function createUserId(id: string): UserId {
  return id as UserId;
}

const userId = createUserId('user-123');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>User ID: {userId}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-branded-types/`
- [ ] Uses branded type with intersection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases brand without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
