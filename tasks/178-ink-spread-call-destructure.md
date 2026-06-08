# Task 178: `ink-spread-call-destructure` Example — Spread in Function Calls, Destructuring in Params/Catch/For-Of

**Priority:** P1-High
**Phase:** 16 — Syntax Feature Completion
**Depends on:** 177

## Problem

Spread in function calls (`fn(...args)`), destructuring in parameters (`fn({a})`), destructuring in catch (`catch ({message})`), and destructuring in for-of (`for (const {a} of arr)`) are syntax features not explicitly covered by any Ink example.

## Ink Example

```tsx
// examples/ink-spread-call-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function sum(...nums: number[]): number {
  return nums.reduce((a, b) => a + b, 0);
}

function greet({ name, age }: { name: string; age: number }): string {
  return `Hello ${name}, age ${age}`;
}

const data = [
  { name: 'Alice', age: 30 },
  { name: 'Bob', age: 25 },
];

let caught = '';
try {
  throw { message: 'test error', code: 42 };
} catch ({ message, code }) {
  caught = `${message} (${code})`;
}

const names: string[] = [];
for (const { name } of data) {
  names.push(name);
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum: {sum(1, 2, 3, 4)}</Text>
      <Text>{greet({ name: 'Alice', age: 30 })}</Text>
      <Text>Caught: {caught}</Text>
      <Text>Names: {names.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-spread-call-destructure/`
- [ ] Uses spread in function call
- [ ] Uses destructuring in function parameter
- [ ] Uses destructuring in catch clause
- [ ] Uses destructuring in for-of loop
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
