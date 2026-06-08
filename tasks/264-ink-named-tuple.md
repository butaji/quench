# Task 264: `ink-named-tuple` Example — Named Tuple Members

**Priority:** P1-High
**Phase:** 22 — TypeScript Type Patterns
**Depends on:** 263

## Problem

Named tuple members (`[x: number, y: number]`) allow naming tuple positions for documentation and tooling. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-named-tuple/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function getPoint(): [x: number, y: number] {
  return [10, 20];
}

export default function App() {
  const [x, y] = getPoint();

  return (
    <Box flexDirection="column">
      <Text>X: {x}</Text>
      <Text>Y: {y}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-named-tuple/`
- [ ] Uses named tuple member syntax `[name: type]`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases named tuple names without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
