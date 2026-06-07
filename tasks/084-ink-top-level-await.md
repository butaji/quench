# Task 084: `ink-top-level-await` Example — Top-Level Await

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Top-level `await` (ES2022) allows `await` at the module level without wrapping in an async function. This is common in app entry points for config loading, data fetching, or dynamic imports. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-top-level-await/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

// Top-level await for simulated config loading
const config = await Promise.resolve({ theme: 'dark', lang: 'en' });

// Top-level await with dynamic import
const { default: helper } = await import('./helper.js');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Theme: {config.theme}</Text>
      <Text>Lang: {config.lang}</Text>
      <Text>Helper: {helper()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-top-level-await/`
- [ ] Uses top-level `await` without async function wrapper
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust (wraps module in async block)
- [ ] Parity harness passes with 100% match in all 3 environments