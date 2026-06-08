# Task 265: `ink-const-enum` Example — `const enum` Declaration

**Priority:** P1-High
**Phase:** 22 — TypeScript Type Patterns
**Depends on:** 264

## Problem

`const enum` declarations are inlined at compile time rather than emitted as runtime objects. Task 216 covers `preserveConstEnums` but no dedicated example exercises plain `const enum`.

## Ink Example

```tsx
// examples/ink-const-enum/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const enum Direction {
  Up = 'UP',
  Down = 'DOWN',
  Left = 'LEFT',
  Right = 'RIGHT',
}

export default function App() {
  const dir = Direction.Right;

  return (
    <Box flexDirection="column">
      <Text>Direction: {dir}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-const-enum/`
- [ ] Uses `const enum` declaration
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path inlines or handles `const enum`
- [ ] Parity harness passes with 100% match in all 3 environments
