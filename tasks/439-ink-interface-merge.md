# Task 439: `ink-interface-merge` Example — Interface Merging Across Declarations

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 438

## Problem

TypeScript allows multiple declarations of the same interface to be merged into a single definition. Task 384 covers `interface extends A, B, C` but not declaration merging. This exercises HIR's ability to handle duplicate interface declarations without collapsing to `Stmt::Invalid`.

## HIR Coverage

- `Stmt::Interface` with duplicate names
- HIR must accept multiple interface declarations with the same identifier
- No runtime codegen needed (type erasure)

## Compile-Path Codegen

- Type erasure: all interface declarations are stripped

## Ink Example

```tsx
// examples/ink-interface-merge/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Person {
  name: string;
}

interface Person {
  age: number;
}

const person: Person = { name: 'Alice', age: 30 };

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {person.name}</Text>
      <Text>Age: {person.age}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-interface-merge/`
- [ ] Uses merged interface declarations
- [ ] HIR parser accepts duplicate interface names without `Stmt::Invalid`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
