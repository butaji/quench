# Task 213: `ink-jsx-factory-pragma` Example — `jsxFactory`, `jsxFragmentFactory`, `jsxImportSource` Pragmas

**Priority:** P1-High
**Phase:** 19 — TypeScript Configuration
**Depends on:** 212

## Problem

JSX factory pragmas (`/** @jsx jsx */`, `/** @jsxFrag Fragment */`, `/** @jsxImportSource react */`) control how JSX is transformed. Task 139 covers jsx pragma handling but no existing example exercises all three pragmas.

## Ink Example

```tsx
/** @jsxImportSource react */
/** @jsxFrag React.Fragment */

// examples/ink-jsx-factory-pragma/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <>
      <Box flexDirection="column">
        <Text>JSX pragma example</Text>
      </Box>
    </>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-jsx-factory-pragma/`
- [ ] Uses `/** @jsxImportSource react */` pragma
- [ ] Uses `/** @jsxFrag React.Fragment */` pragma
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects JSX pragmas during transpilation
- [ ] Parity harness passes with 100% match in all 3 environments
