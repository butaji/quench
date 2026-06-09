# Task 231: `ink-verbatim-module-syntax` Example — `verbatimModuleSyntax` (TS 5.0)

**Priority:** P1-High
**Phase:** 20 — TypeScript Configuration
**Depends on:** 230

## Problem

`verbatimModuleSyntax` (TypeScript 5.0) ensures that imports/exports are not transformed, and type-only imports are explicitly marked with `type`. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with verbatimModuleSyntax: true
// examples/ink-verbatim-module-syntax/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import type { ReactNode } from 'react';

function Wrapper({ children }: { children: ReactNode }) {
  return <Box>{children}</Box>;
}

export default function App() {
  return (
    <Wrapper>
      <Text>verbatimModuleSyntax example</Text>
    </Wrapper>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- `Import`, `Export`, `ExportAll` statement variants

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- `quote_codegen_stmts.inc` + bundler for module resolution

## Acceptance Criteria

- [ ] Example exists at `examples/ink-verbatim-module-syntax/`
- [ ] Uses `import type` syntax
- [ ] Includes `tsconfig.json` with `verbatimModuleSyntax: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `verbatimModuleSyntax`
- [ ] Parity harness passes with 100% match in all 3 environments
