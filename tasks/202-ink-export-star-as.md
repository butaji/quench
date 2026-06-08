# Task 202: `ink-export-star-as` Example — `export * as ns from "module"`

**Priority:** P1-High
**Phase:** 17 — Module Patterns
**Depends on:** 201

## Problem

`export * as namespace from "module"` is an ES2020 module re-export pattern. Task 141 covers namespace re-export but not the `export * as` syntax. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-export-star-as/lib.ts
export const a = 1;
export const b = 2;
export function greet(): string { return 'hello'; }

// examples/ink-export-star-as/index.ts
export * as Lib from './lib.ts';

// examples/ink-export-star-as/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import { Lib } from './index.ts';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>A: {Lib.a}</Text>
      <Text>B: {Lib.b}</Text>
      <Text>Greet: {Lib.greet()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-export-star-as/`
- [ ] Uses `export * as Lib from './lib'` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `export * as` module re-export
- [ ] Parity harness passes with 100% match in all 3 environments
