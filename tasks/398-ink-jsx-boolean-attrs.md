# Task 398: `ink-jsx-boolean-attrs` Example — JSX Boolean Attributes

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 397

## Problem

JSX boolean attributes (`<Box flexDirection />` equivalent to `flexDirection={true}`) are a common shorthand. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `JsxAttribute` with boolean shorthand (no `={...}`).
- The parser must infer `true` as the attribute value.

## Compile-Path Codegen

- `quote_codegen.rs` JSX element codegen must handle boolean shorthand attributes.
- Shorthand attributes map to `true` in generated Rust.

## Ink Example

```tsx
// examples/ink-jsx-boolean-attrs/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Props {
  showLabel?: boolean;
  showValue?: boolean;
}

function Display({ showLabel, showValue }: Props) {
  return (
    <Box flexDirection="column">
      {showLabel && <Text>Label</Text>}
      {showValue && <Text>Value</Text>}
    </Box>
  );
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Display showLabel showValue />
      <Display showValue />
      <Display />
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-boolean-attrs/`
- [ ] Uses JSX boolean attribute shorthand (`<Component attr />`)
- [ ] HIR `JsxAttribute` with boolean value produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
