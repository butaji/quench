# Task 443: `ink-enum-const-inline` Example — `const enum` Inline Expansion Behavior

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 442

## Problem

`const enum` members are inlined at compile time in TypeScript. Task 265 covers `const enum` declarations but does not explicitly test the inline expansion behavior (i.e., that `MyEnum.A` becomes the literal value at compile time). This is important for compile-path codegen because const enums should not emit runtime enum objects.

## HIR Coverage

- `Stmt::Enum` with `is_const: true`
- `Expr::Member` for const enum member access must be resolved to literals
- No runtime enum object should be emitted

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for enum declaration (erased for const enums)
- `quote_codegen_exprs.inc` for enum member access (inlined to literal)

## Ink Example

```tsx
// examples/ink-enum-const-inline/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const enum Status {
  Idle = 0,
  Loading = 1,
  Done = 2,
}

const current = Status.Loading;
const next = Status.Done;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Current: {current}</Text>
      <Text>Next: {next}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-enum-const-inline/`
- [ ] Uses `const enum` with member access
- [ ] Compile path inlines const enum values (no runtime enum object)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Parity harness passes with 100% match in all 3 environments
