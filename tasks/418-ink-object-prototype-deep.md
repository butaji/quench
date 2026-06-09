# Task 418: `ink-object-prototype-deep` Example — `toLocaleString`, `isPrototypeOf`, `propertyIsEnumerable`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 417

## Problem

`Object.prototype` methods (`toLocaleString`, `isPrototypeOf`, `propertyIsEnumerable`) are not explicitly exercised. These are fundamental prototype methods used in object introspection.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `obj.toLocaleString()`, `obj.isPrototypeOf()`, `obj.propertyIsEnumerable()`
- `Expr::Member` for prototype chain access

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-object-prototype-deep/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Base {}
class Derived extends Base {}

const d = new Derived();
const isProto = Base.prototype.isPrototypeOf(d);
const isEnum = d.propertyIsEnumerable('constructor');
const num = (12345).toLocaleString('en-US');
const date = new Date('2024-06-15').toLocaleDateString('en-US');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>IsPrototype: {isProto ? 'yes' : 'no'}</Text>
      <Text>IsEnumerable: {isEnum ? 'yes' : 'no'}</Text>
      <Text>LocaleNum: {num}</Text>
      <Text>LocaleDate: {date}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-prototype-deep/`
- [ ] Uses `toLocaleString`, `isPrototypeOf`, `propertyIsEnumerable`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
