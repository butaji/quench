# Task 298: `ink-optional-catch-binding` Example — Optional Catch Binding

**Priority:** P1-High
**Phase:** 24 — Language Features
**Depends on:** 297

## Problem

Optional catch binding (`try { ... } catch { ... }`) allows catch clauses without an error parameter (ES2019). No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-optional-catch-binding/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function risky(): string {
  throw new Error('fail');
}

export default function App() {
  let ok = true;
  try {
    risky();
  } catch {
    ok = false;
  }

  return (
    <Box flexDirection="column">
      <Text>OK: {String(ok)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-optional-catch-binding/`
- [ ] Uses `catch` without binding
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for optional catch binding
- [ ] Parity harness passes with 100% match in all 3 environments
