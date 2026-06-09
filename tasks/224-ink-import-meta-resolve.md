# Task 224: `ink-import-meta-resolve` Example — `import.meta.resolve`

**Priority:** P1-High
**Phase:** 20 — Advanced Language Features
**Depends on:** 223

## Problem

`import.meta.resolve(specifier)` resolves a module specifier relative to the current module. Task 169 covers `import.meta.url` but not `import.meta.resolve`.

## Ink Example

```tsx
// examples/ink-import-meta-resolve/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const resolved = import.meta.resolve('./app.tsx');

  return (
    <Box flexDirection="column">
      <Text>Resolved: {resolved}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- `Stmt` variants for control flow and declarations
- `Import`, `Export`, `ExportAll` statement variants

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- `quote_codegen_stmts.inc` for statement codegen
- `quote_codegen_stmts.inc` + bundler for module resolution
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-meta-resolve/`
- [ ] Uses `import.meta.resolve()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `import.meta.resolve`
- [ ] Parity harness passes with 100% match in all 3 environments
