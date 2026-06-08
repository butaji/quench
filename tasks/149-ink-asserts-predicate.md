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

- [x] Example exists at `examples/ink-asserts-predicate/`
- [x] Uses `asserts value is Type` type predicate
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path erases `asserts` predicate without runtime impact
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created comprehensive example demonstrating:
- `asserts value is Type` syntax for type narrowing
- Assert functions that throw on failure
- Functions using assert predicates to narrow types
- `assertNonNull<T>` generic assert function
- `assertIsDefined<T>` for undefined checks
- `assertIsArray<T>` for array type checks

Example renders with multiple assert operations showing:
- String operations: `formatUpper("hello world") = HELLO WORLD`
- Number operations: `double(42) = 84`
- Safe operations: `safeLength("test") = 4`, `safeLength([1,2,3]) = 3`
- Defined checks: `after assert: "defined"`

Added `test_ink_asserts_predicate` test to `src/transpile/tests/rq_parity/mod.rs` with expected output assertions.
