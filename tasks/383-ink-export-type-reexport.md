# Task 383: `ink-export-type-reexport` Example — `export type { X } from './module'`

**Priority:** P1-High
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 382

## Problem

TypeScript supports re-exporting types from another module: `export type { X } from './module'`. This exercises the parser's handling of type-only export declarations with module specifiers.

## HIR Coverage

- `Stmt::Export` must capture `type_only` flag along with re-export source module.
- The bundler must resolve re-exported types and strip them without emitting runtime code.

## Compile-Path Codegen

- Bundler module resolution handles re-export declarations.
- No runtime codegen is emitted for type-only re-exports.

## Ink Example

```tsx
// examples/ink-export-type-reexport/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export type { ReactNode } from 'react';
export type { Color } from 'ink';

export default function App() {
  return (
    <Box>
      <Text>Re-exported types</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-export-type-reexport/`
- [ ] Uses `export type { X } from './module'` syntax
- [ ] HIR parser captures type re-exports without producing `Stmt::Invalid`
- [ ] Bundler resolves and strips re-exported types correctly
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
