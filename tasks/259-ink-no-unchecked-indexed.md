# Task 259: `ink-no-unchecked-indexed` Example — `noUncheckedIndexedAccess`

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 258

## Problem

`noUncheckedIndexedAccess` adds `undefined` to the type of index access results. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with noUncheckedIndexedAccess: true
// examples/ink-no-unchecked-indexed/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const arr = ['a', 'b', 'c'];
  const item = arr[0];

  return (
    <Box flexDirection="column">
      <Text>Item: {item ?? 'unknown'}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-no-unchecked-indexed/`
- [ ] Includes `tsconfig.json` with `noUncheckedIndexedAccess: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `noUncheckedIndexedAccess`
- [ ] Parity harness passes with 100% match in all 3 environments
