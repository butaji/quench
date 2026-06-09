# Task 337: `ink-polymorphic-component` Example — Polymorphic Components with `as` Prop

**Priority:** P2-Medium
**Phase:** 27 — React Type Patterns
**Depends on:** 336

## Problem

Polymorphic components (`<Box as="span">`) allow a component to render as different elements while preserving props. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-polymorphic-component/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Polymorphic component example</Text>
      <Text>(Ink uses Box/Text; polymorphic as-prop is typed only)</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-polymorphic-component/`
- [ ] Documents polymorphic `as` prop pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases polymorphic types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
