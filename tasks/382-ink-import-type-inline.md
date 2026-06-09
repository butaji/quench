# Task 382: `ink-import-type-inline` Example — `import type { type X }` (TS 4.5+)

**Priority:** P1-High
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 381

## Problem

TypeScript 4.5+ supports inline type imports within curly braces: `import type { type X, type Y }`. This is syntactically different from `import { type X }` and needs explicit parser handling.

## HIR Coverage

- `Stmt::Import` must capture `type_only` flag along with inline `type` modifiers on individual specifiers.
- The bundler must strip type-only specifiers without affecting value imports in the same declaration.

## Compile-Path Codegen

- Bundler import resolution strips `type` specifiers before module linking.
- No runtime codegen is emitted for type-only imports.

## Ink Example

```tsx
// examples/ink-import-type-inline/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import type { ReactNode } from 'react';
import type { Color } from 'ink';

interface Props {
  label: string;
  color: Color;
}

export default function App() {
  const props: Props = { label: 'demo', color: 'green' };
  return (
    <Box>
      <Text color={props.color}>{props.label}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-type-inline/`
- [ ] Uses `import type { type X }` syntax
- [ ] HIR parser captures inline type modifiers without producing `Stmt::Invalid`
- [ ] Bundler strips type-only specifiers correctly
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
