# Task 097: `ink-template-literal-types` Example — Template Literal Types

**Priority:** P3-Low
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Template literal types (TS 4.1) allow constructing types from string literals using template syntax. They are purely type-level and erased at compile time. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-template-literal-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Color = 'red' | 'green' | 'blue';
type BgColor = `bg-${Color}`;

const bg: BgColor = 'bg-blue';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Background: {bg}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-template-literal-types/`
- [ ] Uses template literal type (`` `prefix-${Union}` ``)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases template literal types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments