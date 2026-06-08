# Task 149: `ink-asserts-predicate` Example — `asserts` Type Predicate

**Priority:** P1-High
**Phase:** 14 — Type System Deep Coverage
**Depends on:** 103

## Problem

`asserts` type predicates (`function assert(x: unknown): asserts x is string`) are a TypeScript feature for narrowing types in a way that affects control flow. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-asserts-predicate/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function assertIsString(value: unknown): asserts value is string {
  if (typeof value !== 'string') {
    throw new TypeError('Expected string');
  }
}

function assertIsNumber(value: unknown): asserts value is number {
  if (typeof value !== 'number') {
    throw new TypeError('Expected number');
  }
}

function format(value: unknown): string {
  assertIsString(value);
  return value.toUpperCase();
}

const data: unknown = 'hello';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{format(data)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-asserts-predicate/`
- [ ] Uses `asserts value is Type` type predicate
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `asserts` predicate without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments