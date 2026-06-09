# Task 384: `ink-interface-multiple-extends` Example — `interface extends A, B, C`

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 383

## Problem

TypeScript interfaces can extend multiple other interfaces: `interface C extends A, B {}`. No existing Ink example explicitly exercises multiple interface inheritance.

## HIR Coverage

- Interface declarations are type-level constructs.
- The parser erases interfaces during HIR conversion.
- Multiple `extends` clauses must not produce parse errors.

## Compile-Path Codegen

- No runtime codegen is required.
- Interfaces are erased at parse time.

## Ink Example

```tsx
// examples/ink-interface-multiple-extends/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Named {
  name: string;
}

interface Aged {
  age: number;
}

interface Person extends Named, Aged {}

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

- [ ] Example exists at `examples/ink-interface-multiple-extends/`
- [ ] Uses `interface extends A, B` syntax
- [ ] Types are erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
