# Task 353: `ink-type-guard-is` Example — User-Defined Type Guards (`is`)

**Priority:** P1-High
**Phase:** 28 — Advanced Type System Patterns
**Depends on:** 352

## Problem

User-defined type guards (`function isString(x: unknown): x is string`) narrow types at runtime. Task 103 covers type guards but not an explicit `is` predicate example.

## Ink Example

```tsx
// examples/ink-type-guard-is/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function isString(x: unknown): x is string {
  return typeof x === 'string';
}

function isNumber(x: unknown): x is number {
  return typeof x === 'number';
}

export default function App() {
  const values: unknown[] = ['hello', 42, true];
  const strings = values.filter(isString);
  const numbers = values.filter(isNumber);

  return (
    <Box flexDirection="column">
      <Text>Strings: {strings.join(', ')}</Text>
      <Text>Numbers: {numbers.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-type-guard-is/`
- [ ] Uses `is` type predicate
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `is` predicate without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
