# Task 284: `ink-watch-mode` Example — Watch Mode / Incremental Compilation

**Priority:** P2-Medium
**Phase:** 23 — Compile Path Infrastructure
**Depends on:** 283

## Problem

Watch mode and incremental compilation reduce rebuild times during development. No existing task covers `runts build --watch` or incremental compilation.

## Ink Example

```tsx
// examples/ink-watch-mode/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Watch mode example</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-watch-mode/`
- [ ] `runts build --watch` rebuilds on file changes
- [ ] Incremental compilation reuses previous build artifacts
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Parity harness passes with 100% match in all 3 environments
