# Task 082: `ink-type-alias-interface` Example — Type Aliases and Interfaces

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Type aliases (`type`) and interfaces are the two primary TS type declaration forms. No existing Ink example explicitly exercises both in a real TUI context. The compile path must erase these completely without runtime impact.

## Ink Example

```tsx
// examples/ink-type-alias-interface/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// Type alias
type Color = 'red' | 'green' | 'blue';

// Interface
interface User {
  name: string;
  age: number;
  favoriteColor: Color;
}

// Interface extension
interface Admin extends User {
  role: 'admin' | 'super';
}

const admin: Admin = {
  name: 'Alice',
  age: 30,
  favoriteColor: 'blue',
  role: 'admin'
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {admin.name}</Text>
      <Text>Role: {admin.role}</Text>
      <Text>Color: {admin.favoriteColor}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-type-alias-interface/`
- [ ] Uses `type` aliases and `interface` declarations
- [ ] Uses interface extension (`extends`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases all type declarations without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
