# Task 258: `ink-no-fallthrough` Example — `noFallthroughCasesInSwitch`

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 257

## Problem

`noFallthroughCasesInSwitch` reports errors for switch cases that fall through to the next case. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with noFallthroughCasesInSwitch: true
// examples/ink-no-fallthrough/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function describe(n: number): string {
  switch (n) {
    case 1:
      return 'one';
    case 2:
      return 'two';
    default:
      return 'other';
  }
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{describe(2)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-no-fallthrough/`
- [ ] Includes `tsconfig.json` with `noFallthroughCasesInSwitch: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `noFallthroughCasesInSwitch`
- [ ] Parity harness passes with 100% match in all 3 environments
