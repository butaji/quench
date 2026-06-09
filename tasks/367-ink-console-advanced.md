# Task 367: `ink-console-advanced` Example — `console.assert`, `count`, `group`, `trace`, `timeLog`

**Priority:** P2-Medium
**Phase:** 29 — Console API Completion
**Depends on:** 366

## Problem

Advanced `console` methods (`assert`, `count`, `countReset`, `group`, `groupEnd`, `groupCollapsed`, `trace`, `timeLog`, `dir`, `dirxml`, `profile`, `profileEnd`) are not covered by Task 144 which focuses on basic console methods.

## Ink Example

```tsx
// examples/ink-console-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  console.count('label');
  console.count('label');
  console.countReset('label');
  console.time('timer');
  console.timeLog('timer', 'midpoint');
  console.timeEnd('timer');
  console.assert(true, 'this should not show');

  return (
    <Box flexDirection="column">
      <Text>Console advanced example</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-console-advanced/`
- [ ] Uses `count`, `timeLog`, `assert`, `group`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
