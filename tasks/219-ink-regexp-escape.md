# Task 219: `ink-regexp-escape` Example — `RegExp.escape` (ES2025)

**Priority:** P3-Low
**Phase:** 19 — ES2025 Features
**Depends on:** 218

## Problem

`RegExp.escape` (ES2025) escapes special regex characters in a string. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-regexp-escape/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  // Note: RegExp.escape is Stage 3/ES2025 and may not be available in all runtimes.
  const input = 'hello.world';
  const escaped = (RegExp as any).escape ? (RegExp as any).escape(input) : input.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp(escaped);
  const matches = pattern.test('hello.world');

  return (
    <Box flexDirection="column">
      <Text>Input: {input}</Text>
      <Text>Escaped: {escaped}</Text>
      <Text>Matches: {String(matches)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-escape/`
- [ ] Uses `RegExp.escape` or polyfill
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
