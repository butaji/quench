# Task 444: `ink-module-baseurl` Example — `baseUrl` Module Resolution with Non-Relative Imports

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 443

## Problem

TypeScript's `baseUrl` compiler option enables non-relative module resolution (`import { foo } from 'components/foo'` instead of `'../../components/foo'`). Task 211 covers `paths` mapping but not `baseUrl`. This exercises module resolution with a base directory.

## HIR Coverage

- `Stmt::Import` with non-relative module specifier
- Module resolution logic in the bundler

## Compile-Path Codegen

- `quote_codegen_stmts.inc` + bundler for module resolution

## Ink Example

```tsx
// examples/ink-module-baseurl/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// With baseUrl set to project root, this resolves to ./components/helper.ts
// import { helper } from 'components/helper';

// For this example, we use a relative import but document the baseUrl pattern
import { helper } from './helper';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Helper: {helper()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-module-baseurl/`
- [ ] Documents `baseUrl` resolution pattern
- [ ] HIR parser handles non-relative import specifiers
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
