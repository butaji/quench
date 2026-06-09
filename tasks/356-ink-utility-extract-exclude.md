# Task 356: `ink-utility-extract-exclude` Example — `Extract`, `Exclude`, `NonNullable`

**Priority:** P2-Medium
**Phase:** 28 — TypeScript Utility Types
**Depends on:** 355

## Problem

Utility types `Extract`, `Exclude`, and `NonNullable` filter union types. Task 100 covers `Partial`/`Required`/`Pick`/`Omit`/`Record`/`ReturnType`; no example covers `Extract`/`Exclude`/`NonNullable`.

## Ink Example

```tsx
// examples/ink-utility-extract-exclude/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type T = 'a' | 'b' | 'c' | 'd';
type AB = Extract<T, 'a' | 'b'>;
type CD = Exclude<T, 'a' | 'b'>;
type NotNull = NonNullable<string | null | undefined>;

export default function App() {
  const ab: AB = 'a';
  const cd: CD = 'c';
  const notNull: NotNull = 'value';

  return (
    <Box flexDirection="column">
      <Text>Extract: {ab}</Text>
      <Text>Exclude: {cd}</Text>
      <Text>NonNullable: {notNull}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-utility-extract-exclude/`
- [ ] Uses `Extract`, `Exclude`, `NonNullable`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases utility types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
