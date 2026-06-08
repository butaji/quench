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
  // Note: import.meta.resolve may not be available in all runtimes.
  const resolved = (import.meta as any).resolve
    ? (import.meta as any).resolve('./app.tsx')
    : import.meta.url;

  return (
    <Box flexDirection="column">
      <Text>Resolved: {resolved}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-meta-resolve/`
- [ ] Uses `import.meta.resolve()` or polyfills
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `import.meta.resolve`
- [ ] Parity harness passes with 100% match in all 3 environments
