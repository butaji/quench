# Task 389: `ink-destructure-rest-defaults` Example — Destructuring with Rest and Defaults Combined

**Priority:** P1-High
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 388

## Problem

Destructuring patterns can combine rest elements with default values: `const { a = 1, ...rest } = obj` or `const [a = 1, ...rest] = arr`. No existing Ink example explicitly exercises this combination.

## HIR Coverage

- `Pat::Object` with both `Pat::Default` and `Pat::Rest` members.
- `Pat::Array` with both `Pat::Default` and `Pat::Rest` elements.

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust for object/array destructuring with defaults and rest combined.
- Default values are evaluated when the property is `undefined`.
- Rest elements collect remaining properties into a new object/array.

## Ink Example

```tsx
// examples/ink-destructure-rest-defaults/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const obj = { x: 10, y: 20, z: 30 };
const arr = [1, 2, 3, 4];

export default function App() {
  const { x = 0, y = 0, ...restObj } = obj;
  const [first = 0, second = 0, ...restArr] = arr;

  return (
    <Box flexDirection="column">
      <Text>x={x}, y={y}, rest={JSON.stringify(restObj)}</Text>
      <Text>first={first}, second={second}, rest={restArr.join(',')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-destructure-rest-defaults/`
- [ ] Uses destructuring with both rest and default values
- [ ] HIR `Pat::Object`/`Pat::Array` with defaults and rest produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
