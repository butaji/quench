# Task 209: `ink-object-static` Example — Object Static Methods (is, setPrototypeOf, getPrototypeOf, preventExtensions, etc.)

**Priority:** P2-Medium
**Phase:** 19 — Runtime API Deep Coverage
**Depends on:** 208

## Problem

Advanced Object static methods (`Object.is`, `setPrototypeOf`, `getPrototypeOf`, `preventExtensions`, `isExtensible`, `isFrozen`, `isSealed`, `getOwnPropertyNames`, `getOwnPropertySymbols`) are not covered by any existing task.

## Ink Example

```tsx
// examples/ink-object-static/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: 1 };

  const isSame = Object.is(NaN, NaN);
  const proto = Object.getPrototypeOf(obj);
  const extensible = Object.isExtensible(obj);
  const frozen = Object.isFrozen(obj);
  const sealed = Object.isSealed(obj);
  const names = Object.getOwnPropertyNames(obj);
  const symbols = Object.getOwnPropertySymbols(obj);

  return (
    <Box flexDirection="column">
      <Text>is(NaN, NaN): {String(isSame)}</Text>
      <Text>isExtensible: {String(extensible)}</Text>
      <Text>isFrozen: {String(frozen)}</Text>
      <Text>isSealed: {String(sealed)}</Text>
      <Text>Names: {names.join(', ')}</Text>
      <Text>Symbols: {symbols.length}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- `ClassMember` and `Class` variants

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- `quote_codegen.rs` for class declaration codegen
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-static/`
- [ ] Uses `Object.is`, `getPrototypeOf`, `isExtensible`, `isFrozen`, `isSealed`, `getOwnPropertyNames`, `getOwnPropertySymbols`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
