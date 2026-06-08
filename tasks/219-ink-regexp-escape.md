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
  const input = 'hello.world';
  const escaped = RegExp.escape(input);
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
- [ ] Uses `RegExp.escape`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
