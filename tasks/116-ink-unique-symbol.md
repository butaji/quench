# Task 116: `ink-unique-symbol` Example — `unique symbol`, Branded Types

**Priority:** P2-Medium
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

`unique symbol` and branded types (nominal typing via symbol or intersection) are advanced TypeScript patterns for creating distinct types that are structurally identical but not interchangeable. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-unique-symbol/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

declare const UserIdBrand: unique symbol;
type UserId = string & { [UserIdBrand]: true };

function createUserId(id: string): UserId {
  return id as UserId;
}

const userId = createUserId('user-123');

// Branded type for currency
type USD = number & { __currency: 'USD' };
function usd(amount: number): USD {
  return amount as USD;
}

const price = usd(99.99);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>User ID: {userId}</Text>
      <Text>Price: ${price.toFixed(2)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-unique-symbol/`
- [ ] Uses `declare const ...: unique symbol`
- [ ] Uses branded type pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `unique symbol` and brands without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
