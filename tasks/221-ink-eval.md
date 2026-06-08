# Task 221: `ink-eval` Example — `eval()` Function

**Priority:** P2-Medium
**Phase:** 20 — Advanced Language Features
**Depends on:** 220

## Problem

`eval()` dynamically executes JavaScript code from a string. It breaks static analysis and must be handled carefully in the compile path. No existing Ink example exercises `eval()`.

## Ink Example

```tsx
// examples/ink-eval/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const expr = '2 + 3';
  const result = eval(expr);

  return (
    <Box flexDirection="column">
      <Text>Expression: {expr}</Text>
      <Text>Result: {result}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-eval/`
- [ ] Uses `eval()` with a string expression
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path either handles `eval()` or produces clear error
- [ ] Parity harness passes with 100% match in all 3 environments
