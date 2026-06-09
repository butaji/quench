# Task 260: `ink-exact-optional` Example — `exactOptionalPropertyTypes`

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 259

## Problem

`exactOptionalPropertyTypes` distinguishes between `undefined` and absent optional properties. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with exactOptionalPropertyTypes: true
// examples/ink-exact-optional/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Props {
  name: string;
  suffix?: string;
}

function Greet({ name, suffix }: Props) {
  return <Text>{name}{suffix ? ` ${suffix}` : ''}</Text>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Greet name="Hello" />
      <Greet name="Hi" suffix="there" />
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

- [ ] Example exists at `examples/ink-exact-optional/`
- [ ] Includes `tsconfig.json` with `exactOptionalPropertyTypes: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `exactOptionalPropertyTypes`
- [ ] Parity harness passes with 100% match in all 3 environments
