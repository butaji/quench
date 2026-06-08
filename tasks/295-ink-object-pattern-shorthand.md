# Task 295: `ink-object-pattern-shorthand` Example — Shorthand Properties and Method Syntax

**Priority:** P1-High
**Phase:** 24 — Expression Patterns
**Depends on:** 294

## Problem

Object shorthand properties (`{ a }` instead of `{ a: a }`) and method shorthand (`{ method() {} }`) are core JavaScript features. No dedicated example exercises them.

## Ink Example

```tsx
// examples/ink-object-pattern-shorthand/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const name = 'Alice';
const age = 30;

const person = {
  name,
  age,
  greet() {
    return `Hello, ${this.name}`;
  },
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {person.name}</Text>
      <Text>Age: {person.age}</Text>
      <Text>Greeting: {person.greet()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-pattern-shorthand/`
- [ ] Uses object shorthand properties
- [ ] Uses method shorthand syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
