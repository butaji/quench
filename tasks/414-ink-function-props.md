# Task 414: `ink-function-props` Example — `length`, `name`, `prototype`, `[Symbol.hasInstance]`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 413

## Problem

Function object properties (`length`, `name`, `prototype`) and `Symbol.hasInstance` are important introspection features. No existing Ink example exercises these Function object properties.

## HIR Coverage

- `Expr::Member` for `fn.length`, `fn.name`, `fn.prototype`
- `Expr::ComputedMember` for `fn[Symbol.hasInstance]`
- `Expr::New` with class constructors to test `instanceof`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-function-props/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(a: string, b: string, c?: string): string {
  return `${a} ${b}`;
}

class MyClass {}

const len = greet.length;
const name = greet.name;
const hasProto = greet.prototype !== undefined;
const isInstance = MyClass[Symbol.hasInstance](new MyClass());

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Length: {len}</Text>
      <Text>Name: {name}</Text>
      <Text>HasPrototype: {hasProto ? 'yes' : 'no'}</Text>
      <Text>IsInstance: {isInstance ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-props/`
- [ ] Uses `Function.prototype.length`, `Function.prototype.name`, `Function.prototype.prototype`, `Function.prototype[Symbol.hasInstance]`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
