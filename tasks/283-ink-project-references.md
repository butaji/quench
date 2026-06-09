# Task 283: `ink-project-references` Example — TypeScript Project References

**Priority:** P2-Medium
**Phase:** 23 — TypeScript Configuration
**Depends on:** 282

## Problem

TypeScript project references enable splitting a codebase into smaller buildable units. No existing Ink example exercises this feature.

## Ink Example

```tsx
// examples/ink-project-references/tsconfig.json
{
  "references": [
    { "path": "./shared" },
    { "path": "./app" }
  ]
}

// examples/ink-project-references/shared/tsconfig.json + index.ts
export const version = '1.0.0';

// examples/ink-project-references/app/tsconfig.json + app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import { version } from '../shared';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Version: {version}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Import`, `Export`, `ExportAll` statement variants

## Compile-Path Codegen

- `quote_codegen_stmts.inc` + bundler for module resolution

## Acceptance Criteria

- [ ] Example exists at `examples/ink-project-references/`
- [ ] Uses `tsconfig.json` with `references` and `composite: true`
- [ ] Imports between referenced projects
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects project references
- [ ] Parity harness passes with 100% match in all 3 environments
