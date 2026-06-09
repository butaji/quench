# Task 410: `ink-object-static-deep` Example — `getOwnPropertyDescriptor`, `preventExtensions`, `isExtensible`, `isSealed`, `isFrozen`, `getOwnPropertyNames`, `getOwnPropertySymbols`, `defineProperties`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 409

## Problem

Advanced `Object` static methods (`getOwnPropertyDescriptor`, `preventExtensions`, `isExtensible`, `isSealed`, `isFrozen`, `getOwnPropertyNames`, `getOwnPropertySymbols`, `defineProperties`) are important for meta-programming and property introspection. Tasks 105, 123, 166, 183, 209, and 362 cover some Object statics, but the descriptor and integrity methods are not explicitly exercised.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `Object.*` static methods
- `Expr::Object` for property descriptors passed to `defineProperties`
- `Expr::Array` for results from `getOwnPropertyNames` / `getOwnPropertySymbols`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-object-static-deep/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const obj: Record<string, unknown> = { a: 1, b: 2 };
Object.defineProperties(obj, {
  c: { value: 3, writable: true, enumerable: true, configurable: true },
});

const desc = Object.getOwnPropertyDescriptor(obj, 'c');
const names = Object.getOwnPropertyNames(obj);
const symbols = Object.getOwnPropertySymbols(obj);
const extensible = Object.isExtensible(obj);

Object.preventExtensions(obj);
const sealed = Object.isSealed(obj);
const frozen = Object.isFrozen(obj);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Names: {names.join(', ')}</Text>
      <Text>Desc value: {String(desc?.value)}</Text>
      <Text>Symbols: {symbols.length}</Text>
      <Text>Extensible: {String(extensible)}</Text>
      <Text>Sealed: {String(sealed)}</Text>
      <Text>Frozen: {String(frozen)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-static-deep/`
- [ ] Uses `getOwnPropertyDescriptor`, `preventExtensions`, `isExtensible`, `isSealed`, `isFrozen`, `getOwnPropertyNames`, `getOwnPropertySymbols`, `defineProperties`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
