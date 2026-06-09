# Task 434: `ink-process-args` Example — `process.argv`, `process.title`, `process.execPath`, `process.execArgv`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 433

## Problem

`process.argv`, `process.title`, `process.execPath`, and `process.execArgv` are commonly accessed Node.js process properties. Tasks 133, 323, and 412 cover other `process` properties but not these specific ones.

## HIR Coverage

- `Expr::Member` for `process.argv`, `process.title`, `process.execPath`, `process.execArgv`
- `Expr::Member` for `process.argv.length`, `process.argv[0]`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-process-args/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const args = process.argv.slice(2);
const title = process.title;
const execPath = process.execPath;
const execArgv = process.execArgv;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Args: {args.join(', ') || 'none'}</Text>
      <Text>Title: {title}</Text>
      <Text>Exec: {execPath}</Text>
      <Text>ExecArgv: {execArgv.join(', ') || 'none'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-process-args/`
- [ ] Uses `process.argv`, `process.title`, `process.execPath`, `process.execArgv`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
