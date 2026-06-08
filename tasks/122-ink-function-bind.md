# Task 122: `ink-function-bind` Example — `bind`, `call`, `apply`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 121

## Problem

`Function.prototype.bind`, `call`, and `apply` are fundamental JavaScript methods for controlling `this` binding and argument passing. No existing Ink example explicitly exercises all three.

## Ink Example

```tsx
// examples/ink-function-bind/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(this: { name: string }, greeting: string): string {
  return `${greeting}, ${this.name}!`;
}

const alice = { name: 'Alice' };
const bob = { name: 'Bob' };

const greetAlice = greet.bind(alice);
const callResult = greet.call(bob, 'Hello');
const applyResult = greet.apply(alice, ['Hi']);

function sum(a: number, b: number, c: number): number {
  return a + b + c;
}

const partial = sum.bind(null, 1);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Bind: {greetAlice('Hey')}</Text>
      <Text>Call: {callResult}</Text>
      <Text>Apply: {applyResult}</Text>
      <Text>Partial: {partial(2, 3)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-bind/`
- [ ] Uses `Function.prototype.bind`
- [ ] Uses `Function.prototype.call`
- [ ] Uses `Function.prototype.apply`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
