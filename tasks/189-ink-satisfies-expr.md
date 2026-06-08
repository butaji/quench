# Task 189: `ink-satisfies-expr` Example — `satisfies` in Object/Array Literals

**Priority:** P1-High
**Phase:** 17 — TypeScript 4.9+ Features
**Depends on:** 188

## Problem

`satisfies` operator in object/array literals (`{ x: 1 } satisfies Record<string, number>`) is a TypeScript 4.9 feature for type checking without widening. Task 066/069 partially cover `satisfies` but not in object/array literal contexts. No existing Ink example explicitly exercises this pattern.

## Ink Example

```tsx
// examples/ink-satisfies-expr/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const config = {
  theme: 'dark',
  width: 80,
} as const satisfies Record<string, string | number>;

const colors = ['red', 'green', 'blue'] as const satisfies readonly string[];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Theme: {config.theme}</Text>
      <Text>Colors: {colors.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-satisfies-expr/`
- [ ] Uses `satisfies` on object literal
- [ ] Uses `satisfies` on array literal
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `satisfies` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
