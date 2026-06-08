# Task 266: `ink-ambient-module` Example — Ambient Module Declarations

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Type Patterns
**Depends on:** 265

## Problem

Ambient module declarations (`declare module "foo" { ... }`) provide type information for untyped modules. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-ambient-module/types.d.ts
declare module 'my-lib' {
  export function greet(name: string): string;
}

// examples/ink-ambient-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// The module is not actually loaded; this exercises the ambient declaration.
declare const myGreet: (name: string) => string;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Ambient module example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ambient-module/`
- [ ] Uses `declare module "..."` in ambient declaration file
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips ambient module declarations
- [ ] Parity harness passes with 100% match in all 3 environments
