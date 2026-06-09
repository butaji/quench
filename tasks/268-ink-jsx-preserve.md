# Task 268: `ink-jsx-preserve` Example — JSX `preserve` Transform

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 267

## Problem

TypeScript's `jsx: "preserve"` option keeps JSX syntax in the output instead of transforming it. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with jsx: "preserve"
// examples/ink-jsx-preserve/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>JSX preserve example</Text>
    </Box>
  );
}
```


## HIR Coverage

- `JsxElement`, `JsxFragment`, `JsxSpreadAttribute` variants
- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- `quote_codegen.rs` JSX element codegen + Ratatui plugin
- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-preserve/`
- [ ] Includes `tsconfig.json` with `jsx: "preserve"`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `jsx: "preserve"` setting
- [ ] Parity harness passes with 100% match in all 3 environments
