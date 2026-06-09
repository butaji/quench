# Task 411: `ink-console-debug` Example — `console.debug`, `dir`, `groupEnd`, `countReset`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 410

## Problem

Additional `console` methods (`debug`, `dir`, `groupEnd`, `countReset`) are commonly used for debugging. Task 144 covers basic console methods and Task 367 covers advanced methods (`assert`, `count`, `group`, `trace`, `timeLog`), but `debug`, `dir`, `groupEnd`, and `countReset` are not explicitly exercised.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `console.*` methods
- `Expr::Object` for complex data passed to `dir`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-console-debug/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

console.debug('debug message');
console.dir({ a: 1, nested: { b: 2 } }, { depth: 2 });
console.count('label');
console.count('label');
console.countReset('label');
console.group('group1');
console.groupEnd();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Console debug example complete</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-console-debug/`
- [ ] Uses `debug`, `dir`, `groupEnd`, `countReset`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
