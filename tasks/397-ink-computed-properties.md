# Task 397: `ink-computed-properties` Example — Computed Property Names `[expr]: value`

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 396

## Problem

Computed property names (`{[expr]: value}`) allow dynamic keys in object literals. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Property::Computed` with `Expr` key and `Expr` value.
- Dynamic key evaluation at object creation time.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for computed property keys.
- Keys must be evaluated before object construction.

## Ink Example

```tsx
// examples/ink-computed-properties/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const prefix = 'item';
const id = 42;

const obj = {
  [`${prefix}_1`]: 'first',
  [`${prefix}_2`]: 'second',
  [String(id)]: 'forty-two',
  [prefix + '_count']: 3
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{item_1}: {obj.item_1}</Text>
      <Text>{item_2}: {obj.item_2}</Text>
      <Text>42: {obj['42']}</Text>
      <Text>Count: {obj.item_count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-computed-properties/`
- [ ] Uses computed property names `[expr]: value`
- [ ] HIR `Property::Computed` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
