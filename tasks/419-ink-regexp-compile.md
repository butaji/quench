# Task 419: `ink-regexp-compile` Example — `RegExp.prototype.compile`, `RegExp.prototype.toString`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 418

## Problem

`RegExp.prototype.compile` and `RegExp.prototype.toString` are not explicitly exercised. Tasks 99, 160, 175, and 364 cover other RegExp features, but these prototype methods are missing.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `regex.compile()` and `regex.toString()`
- `Expr::New` for `RegExp` constructor

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-regexp-compile/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const re = new RegExp('foo', 'g');
const before = re.toString();
re.compile('bar', 'i');
const after = re.toString();
const test = re.test('BAR');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Before: {before}</Text>
      <Text>After: {after}</Text>
      <Text>Test: {test ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-compile/`
- [ ] Uses `RegExp.prototype.compile` and `RegExp.prototype.toString`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
