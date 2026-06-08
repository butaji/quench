# Task 170: `ink-debugger-labeled` Example — `debugger` Statement, Labeled Statements

**Priority:** P2-Medium
**Phase:** 16 — Statement Completion
**Depends on:** 164

## Problem

`debugger` statement and labeled statements (`label: while (...)`) are JavaScript control flow features. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-debugger-labeled/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  let result = '';

  outer: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if (i === 1 && j === 1) {
        break outer;
      }
      result += `${i}${j} `;
    }
  }

  // debugger; // Would pause in dev tools, stripped in production

  return (
    <Box flexDirection="column">
      <Text>Result: {result.trim()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-debugger-labeled/`
- [ ] Uses labeled statement with `break`
- [ ] Uses `debugger` statement (commented or guarded)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `debugger` and labeled statements
- [ ] Parity harness passes with 100% match in all 3 environments
