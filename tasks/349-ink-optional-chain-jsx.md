# Task 349: `ink-optional-chain-jsx` Example — Optional Chaining in JSX Attributes

**Priority:** P1-High
**Phase:** 27 — JSX Expression Patterns
**Depends on:** 348

## Problem

Optional chaining inside JSX attributes (`color={theme?.primary ?? 'white'}`) safely accesses nested properties. No dedicated Ink example exercises this.

## Ink Example

```tsx
// examples/ink-optional-chain-jsx/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Theme {
  primary?: string;
}

export default function App() {
  const theme: Theme | undefined = { primary: 'cyan' };

  return (
    <Box flexDirection="column">
      <Text color={theme?.primary ?? 'white'}>Themed text</Text>
      <Text>Length: {theme?.primary?.length ?? 0}</Text>
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

## Acceptance Criteria

- [ ] Example exists at `examples/ink-optional-chain-jsx/`
- [ ] Uses `?.` in JSX attribute
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
