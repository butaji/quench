# Task 197: `ink-ts-directive` Example — `// @ts-expect-error` and `// @ts-ignore`

**Priority:** P2-Medium
**Phase:** 17 — TypeScript Compiler Directives
**Depends on:** 196

## Problem

TypeScript directive comments (`// @ts-expect-error`, `// @ts-ignore`, `// @ts-nocheck`, `// @ts-check`) control compiler behavior but are erased at runtime. No existing Ink example exercises these directives.

## Ink Example

```tsx
// examples/ink-ts-directive/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function getValue(): string {
  return 'hello';
}

export default function App() {
  // @ts-expect-error — deliberate type mismatch for testing
  const num: number = getValue();

  // @ts-ignore — suppress next line
  const bool: boolean = 'not-a-boolean' as any;

  return (
    <Box flexDirection="column">
      <Text>Value: {num}</Text>
      <Text>Bool: {String(bool)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ts-directive/`
- [ ] Uses `// @ts-expect-error` directive
- [ ] Uses `// @ts-ignore` directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips directives without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
