# Task 446: `ink-function-bind-chain` Example — Chained `Function.prototype.bind` Calls

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 445

## Problem

Chained `.bind()` calls (`fn.bind(obj1).bind(obj2)`) exercise HIR member access and call expression chains with multiple closures. Task 122 covers `bind`, `call`, `apply` but not chained binds.

## HIR Coverage

- `Expr::Call` with chained `Expr::Member` callees (`fn.bind(obj1).bind(obj2)`)
- Multiple closure creation from chained binds

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for method call expressions
- Runtime API mapping for `Function.prototype.bind`

## Ink Example

```tsx
// examples/ink-function-bind-chain/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(greeting: string, punctuation: string): string {
  return `${greeting}, ${this.name}${punctuation}`;
}

const obj1 = { name: 'World' };
const obj2 = { name: 'Universe' };

const bound1 = greet.bind(obj1);
const bound2 = bound1.bind(obj2);
const partial = greet.bind(obj1, 'Hello');

const result1 = bound1('Hi', '!');
const result2 = bound2('Greetings', '.');
const result3 = partial('!');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Bound1: {result1}</Text>
      <Text>Bound2: {result2}</Text>
      <Text>Partial: {result3}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-bind-chain/`
- [ ] Uses chained `.bind()` calls and partial application
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
