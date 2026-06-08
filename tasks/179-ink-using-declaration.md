# Task 179: `ink-using-declaration` Example — `using` and `await using` (ES2024)

**Priority:** P1-High
**Phase:** 17 — ES2024 / TS 5.2 Features
**Depends on:** 178

## Problem

`using` and `await using` declarations (ES2024 / TypeScript 5.2) enable explicit resource management with automatic cleanup via `Symbol.dispose` and `Symbol.asyncDispose`. No existing Ink example exercises these features.

## Ink Example

```tsx
// examples/ink-using-declaration/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const resource = {
  [Symbol.dispose](): void {
    // cleanup
  },
  name: 'Resource',
};

export default function App() {
  using r = resource;

  return (
    <Box flexDirection="column">
      <Text>Resource: {r.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-using-declaration/`
- [ ] Uses `using` declaration with `Symbol.dispose`
- [ ] Optionally uses `await using` with `Symbol.asyncDispose`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
