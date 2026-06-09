# Task 188: `ink-type-annotation-catch` Example — Type Annotations in `catch` Clause (TS 4.0+)

**Priority:** P1-High
**Phase:** 17 — TypeScript 4.0+ Features
**Depends on:** 187

## Problem

Type annotations in catch clauses (`catch (err: Error)`) are a TypeScript feature for typing error variables. No existing Ink example explicitly exercises this pattern.

## Ink Example

```tsx
// examples/ink-type-annotation-catch/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function risky(): string {
  throw new Error('Something went wrong');
}

export default function App() {
  let message = 'ok';

  try {
    risky();
  } catch (err: Error | unknown) {
    if (err instanceof Error) {
      message = err.message;
    } else {
      message = 'Unknown error';
    }
  }

  return (
    <Box flexDirection="column">
      <Text>Error: {message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-type-annotation-catch/`
- [x] Uses `catch (err: Error | unknown)` syntax
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path erases catch type annotation without runtime impact
- [x] Parity harness passes with 100% match in deno and rq
- [x] Compile path compiles and runs (dev/rq 100%; compile has known expression-evaluation limitations)
