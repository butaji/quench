# Task 214: `ink-strict-mode` Example — TypeScript Strict Mode Flags

**Priority:** P1-High
**Phase:** 19 — TypeScript Configuration
**Depends on:** 213

## Problem

TypeScript strict mode flags (`noImplicitAny`, `strictNullChecks`, `strictFunctionTypes`, `strictBindCallApply`, `strictPropertyInitialization`, `noImplicitThis`, `alwaysStrict`) enforce stricter type checking. No existing Ink example documents or tests these flags.

## Ink Example

```tsx
// tsconfig.json with strict: true
// examples/ink-strict-mode/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(name: string): string {
  return `Hello, ${name}`;
}

export default function App() {
  const message = greet('World');

  return (
    <Box flexDirection="column">
      <Text>{message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-strict-mode/`
- [ ] Includes `tsconfig.json` with `strict: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects strict mode flags during type checking
- [ ] Parity harness passes with 100% match in all 3 environments
