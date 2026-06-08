# Task 357: `ink-key-remapping` Example — Key Remapping with Mapped Types

**Priority:** P2-Medium
**Phase:** 28 — Advanced Type System Patterns
**Depends on:** 356

## Problem

Key remapping in mapped types (`[K in keyof T as NewKey<K>]`) transforms object keys. Task 351 covers `as` clause; this example focuses on pure key remapping.

## Ink Example

```tsx
// examples/ink-key-remapping/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type PrefixKeys<T, P extends string> = {
  [K in keyof T as `${P}${string & K}`]: T[K];
};

interface Base {
  name: string;
  value: number;
}

type Prefixed = PrefixKeys<Base, 'meta_';

const data: Prefixed = {
  meta_name: 'example',
  meta_value: 42,
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {data.meta_name}</Text>
      <Text>Value: {data.meta_value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-key-remapping/`
- [ ] Uses key remapping in mapped type
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases mapped types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
