# Task 438: `ink-export-type-star` Example — `export type * from './module'`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 437

## Problem

TypeScript supports `export type * from './module'` for re-exporting all types from another module. Tasks 202 and 383 cover named re-exports and `export type { X } from`, but `export type * from` is a distinct module syntax variant not yet exercised.

## HIR Coverage

- `Stmt::ExportAll` with `type_only: true` flag
- Module resolution and bundler handling for type-only star re-exports

## Compile-Path Codegen

- `quote_codegen_stmts.inc` + bundler for module resolution
- No runtime codegen emitted for type-only re-exports

## Ink Example

```tsx
// examples/ink-export-type-star/types.ts
export interface User {
  name: string;
}

export type ID = string | number;

// examples/ink-export-type-star/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export type * from './types';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Type-only star re-export example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-export-type-star/`
- [ ] Uses `export type * from './module'`
- [ ] HIR parser captures type-only star re-exports without producing `Stmt::Invalid`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
