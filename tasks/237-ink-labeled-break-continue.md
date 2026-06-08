# Task 237: `ink-labeled-break-continue` Example — Labeled `break` and `continue`

**Priority:** P2-Medium
**Phase:** 21 — Niche Language Features
**Depends on:** 236

## Problem

Labeled `break` and `continue` statements allow escaping from nested loops. Task 170 covers `debugger` and labeled statements, but no dedicated example exercises `break label` or `continue label`.

## Ink Example

```tsx
// examples/ink-labeled-break-continue/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  let found = '';

  outer: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if (i === 1 && j === 1) {
        found = `stopped at ${i},${j}`;
        break outer;
      }
    }
  }

  return (
    <Box flexDirection="column">
      <Text>Found: {found}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-labeled-break-continue/`
- [ ] Uses labeled `break` or `continue`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for labeled break/continue
- [ ] Parity harness passes with 100% match in all 3 environments
