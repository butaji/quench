# Task 239: `ink-default-destructure` Example — Default Values in Destructuring

**Priority:** P1-High
**Phase:** 21 — Niche Language Features
**Depends on:** 238

## Problem

Default values in destructuring (`const { x = 1 } = obj`, `const [a = 'default'] = arr`) provide fallback values when extracted properties are `undefined`. No dedicated example exercises this pattern.

## Ink Example

```tsx
// examples/ink-default-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj: { a?: number; b?: string } = { a: 5 };
  const arr: (string | undefined)[] = ['first', undefined, 'third'];

  const { a = 0, b = 'missing' } = obj;
  const [x = 'X', y = 'Y', z = 'Z'] = arr;

  return (
    <Box flexDirection="column">
      <Text>Object: {a}, {b}</Text>
      <Text>Array: {x}, {y}, {z}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-default-destructure/`
- [ ] Uses default values in object destructuring
- [ ] Uses default values in array destructuring
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for destructuring defaults
- [ ] Parity harness passes with 100% match in all 3 environments
