# Task 370: `ink-import-meta-env` Example — `import.meta.env` (Vite Pattern)

**Priority:** P1-High
**Phase:** 29 — Module Meta Patterns
**Depends on:** 369

## Problem

`import.meta.env` is a Vite-specific pattern for accessing build-time environment variables. It is extremely common in modern TypeScript projects. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-import-meta-env/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const mode = import.meta.env.MODE;
  const dev = import.meta.env.DEV;
  const prod = import.meta.env.PROD;

  return (
    <Box flexDirection="column">
      <Text>Mode: {mode}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-meta-env/`
- [ ] Uses `import.meta.env` pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `import.meta.env`
- [ ] Parity harness passes with 100% match in all 3 environments
