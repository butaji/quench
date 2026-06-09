# Task 445: `ink-jsx-key-spread` Example — JSX `key` Prop Combined with Spread Attributes

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 444

## Problem

JSX elements with both a `key` prop and spread attributes (`<Item key={id} {...props} />`) exercise attribute merging in the JSX parser and HIR. Task 162 covers `key` prop, Task 204 covers spread attributes, but the combination is not explicitly exercised.

## HIR Coverage

- `Expr::JsxElement` with both explicit `key` attribute and `JsxSpreadAttribute`
- Attribute ordering and merging in HIR

## Compile-Path Codegen

- `quote_codegen.rs` + Ratatui plugin for JSX element codegen
- Spread attributes must not override explicit `key`

## Ink Example

```tsx
// examples/ink-jsx-key-spread/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const items = [
  { id: 'a', label: 'First' },
  { id: 'b', label: 'Second' },
  { id: 'c', label: 'Third' },
];

export default function App() {
  return (
    <Box flexDirection="column">
      {items.map((item) => (
        <Text key={item.id} {...item}>
          {item.label}
        </Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-key-spread/`
- [ ] Uses `key` prop combined with `{...props}` spread
- [ ] HIR preserves both `key` and spread attributes
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
