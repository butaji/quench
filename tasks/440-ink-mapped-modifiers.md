# Task 440: `ink-mapped-modifiers` Example — Mapped Types with `+readonly`, `-readonly`, `+?`, `-?` Modifiers

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 439

## Problem

TypeScript mapped types support modifier operators: `+readonly` (add readonly), `-readonly` (remove readonly), `+?` (add optional), `-?` (remove optional). Tasks 110, 351, 357, and 358 cover basic mapped types but not explicit modifier syntax.

## HIR Coverage

- Type-level construct (erased at runtime)
- Parser must accept `+readonly`, `-readonly`, `+?`, `-?` in mapped type positions

## Compile-Path Codegen

- Type erasure: no runtime codegen needed

## Ink Example

```tsx
// examples/ink-mapped-modifiers/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Writable<T> = { -readonly [K in keyof T]: T[K] };
type RequiredProps<T> = { [K in keyof T]-?: T[K] };
type ReadonlyOptional<T> = { +readonly [K in keyof T]+?: T[K] };

interface Config {
  readonly name: string;
  age?: number;
}

const writable: Writable<Config> = { name: 'Alice', age: 30 };
writable.name = 'Bob';

const required: RequiredProps<Config> = { name: 'Alice', age: 30 };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {writable.name}</Text>
      <Text>Required name: {required.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-mapped-modifiers/`
- [ ] Uses `+readonly`, `-readonly`, `+?`, `-?` mapped type modifiers
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
