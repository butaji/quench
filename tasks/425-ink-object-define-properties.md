# Task 425: `ink-object-define-properties` Example — `Object.defineProperties`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 424

## Problem

`Object.defineProperties` (plural) is distinct from `Object.defineProperty` (singular, covered in Task 123). It allows defining multiple properties at once and is an important meta-programming API.

## HIR Coverage

- `Expr::Call` for `Object.defineProperties(obj, descriptors)`
- `Expr::Object` for nested property descriptor objects

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-object-define-properties/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const obj: Record<string, unknown> = {};
Object.defineProperties(obj, {
  name: {
    value: 'Test',
    writable: true,
    enumerable: true,
    configurable: true,
  },
  id: {
    value: 42,
    writable: false,
    enumerable: true,
    configurable: false,
  },
});

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {String(obj.name)}</Text>
      <Text>Id: {String(obj.id)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-define-properties/`
- [ ] Uses `Object.defineProperties`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
