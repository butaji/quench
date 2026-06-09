# Task 262: `ink-jsx-spread-children` Example — Spread in JSX Children (`{...children}`)

**Priority:** P1-High
**Phase:** 22 — JSX Advanced Patterns
**Depends on:** 261

## Problem

JSX spread children (`<Component>{...items}</Component>`) allow spreading an array of elements as children. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-jsx-spread-children/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const items = [
  <Text key="1">First</Text>,
  <Text key="2">Second</Text>,
  <Text key="3">Third</Text>,
];

export default function App() {
  return (
    <Box flexDirection="column">
      {...items}
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- `JsxElement`, `JsxFragment`, `JsxSpreadAttribute` variants

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- `quote_codegen.rs` JSX element codegen + Ratatui plugin
- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-spread-children/`
- [ ] Uses JSX spread children `{...items}`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for JSX spread children
- [ ] Parity harness passes with 100% match in all 3 environments
