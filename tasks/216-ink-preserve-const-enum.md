# Task 216: `ink-preserve-const-enum` Example — `preserveConstEnums`

**Priority:** P2-Medium
**Phase:** 19 — TypeScript Configuration
**Depends on:** 215

## Problem

`preserveConstEnums` keeps `const enum` declarations in the emitted code instead of inlining them. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// examples/ink-preserve-const-enum/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const enum Status {
  Idle = 'idle',
  Active = 'active',
  Done = 'done',
}

export default function App() {
  const current = Status.Active;

  return (
    <Box flexDirection="column">
      <Text>Status: {current}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-preserve-const-enum/`
- [ ] Uses `const enum` declaration
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `preserveConstEnums` option
- [ ] Parity harness passes with 100% match in all 3 environments
