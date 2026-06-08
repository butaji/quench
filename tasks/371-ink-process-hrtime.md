# Task 371: `ink-process-hrtime` Example — `process.hrtime` / `process.hrtime.bigint`

**Priority:** P2-Medium
**Phase:** 29 — Node.js Process API
**Depends on:** 370

## Problem

`process.hrtime` provides high-resolution time measurement. `process.hrtime.bigint` returns a BigInt. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-process-hrtime/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const start = process.hrtime.bigint();
  const end = process.hrtime.bigint();
  const diff = Number(end - start);

  return (
    <Box flexDirection="column">
      <Text>Diff (ns): {diff}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-process-hrtime/`
- [ ] References `process.hrtime` / `process.hrtime.bigint`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
