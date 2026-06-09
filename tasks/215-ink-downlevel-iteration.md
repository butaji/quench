# Task 215: `ink-downlevel-iteration` Example — `downlevelIteration` for ES5 Targets

**Priority:** P2-Medium
**Phase:** 19 — TypeScript Configuration
**Depends on:** 214

## Problem

`downlevelIteration` enables accurate iteration of iterables (including strings, `Set`, `Map`) when targeting ES5/ES3. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// examples/ink-downlevel-iteration/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const set = new Set(['a', 'b', 'c']);
  const items: string[] = [];

  for (const item of set) {
    items.push(item);
  }

  return (
    <Box flexDirection="column">
      <Text>Items: {items.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-downlevel-iteration/`
- [ ] Uses `for...of` with a `Set`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles iteration correctly with `downlevelIteration`
- [ ] Parity harness passes with 100% match in all 3 environments
