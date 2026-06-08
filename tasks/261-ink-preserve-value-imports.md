# Task 261: `ink-preserve-value-imports` Example — `preserveValueImports`

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 260

## Problem

`preserveValueImports` preserves imports that are only used for their types. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with preserveValueImports: true
// examples/ink-preserve-value-imports/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import type { ReactNode } from 'react';

function Wrapper({ children }: { children: ReactNode }) {
  return <Box>{children}</Box>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Wrapper>
        <Text>preserveValueImports example</Text>
      </Wrapper>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-preserve-value-imports/`
- [ ] Includes `tsconfig.json` with `preserveValueImports: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `preserveValueImports`
- [ ] Parity harness passes with 100% match in all 3 environments
