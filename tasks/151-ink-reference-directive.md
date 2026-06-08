# Task 151: `ink-reference-directive` Example — `/// <reference types="..." />`

**Priority:** P2-Medium
**Phase:** 14 — Type System Deep Coverage
**Depends on:** 150

## Problem

Triple-slash reference directives (`/// <reference types="..." />`, `/// <reference path="..." />`) are TypeScript's way to declare dependencies on ambient type definitions. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-reference-directive/types.d.ts
/// <reference types="node" />

declare module 'my-lib' {
  export function greet(name: string): string;
}

// examples/ink-reference-directive/tui/app.tsx
/// <reference path="./types.d.ts" />
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Reference directive exercised</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-reference-directive/`
- [ ] Uses `/// <reference types="..." />` directive
- [ ] Uses `/// <reference path="..." />` directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips reference directives without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments