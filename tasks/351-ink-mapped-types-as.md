# Task 351: `ink-mapped-types-as` Example — Mapped Types with `as` Clause

**Priority:** P2-Medium
**Phase:** 28 — Advanced Type System Patterns
**Depends on:** 350

## Problem

Mapped types with `as` clause (`type Getters<T> = { [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K] }`) enable key remapping. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-mapped-types-as/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Getters<T> = {
  [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K];
};

interface Person {
  name: string;
  age: number;
}

const getters: Getters<Person> = {
  getName: () => 'Alice',
  getAge: () => 30,
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {getters.getName()}</Text>
      <Text>Age: {getters.getAge()}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-mapped-types-as/`
- [ ] Uses mapped type with `as` key remapping
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases mapped types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
