# Task 285: `ink-declaration-files` Example — `.d.ts` Declaration Files

**Priority:** P2-Medium
**Phase:** 23 — TypeScript Type System
**Depends on:** 284

## Problem

`.d.ts` declaration files provide type information without runtime code. No existing Ink example exercises writing and importing custom `.d.ts` files.

## Ink Example

```tsx
// examples/ink-declaration-files/types/app.d.ts
declare module 'greeting-lib' {
  export function greet(name: string): string;
}

// examples/ink-declaration-files/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// Ambient declaration used without runtime import
declare function localGreet(name: string): string;

export default function App() {
  const message = localGreet('World');
  return (
    <Box flexDirection="column">
      <Text>{message}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-declaration-files/`
- [ ] Includes custom `.d.ts` file
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips `.d.ts` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
