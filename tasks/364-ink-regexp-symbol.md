# Task 364: `ink-regexp-symbol` Example — `RegExp` Well-Known Symbols

**Priority:** P2-Medium
**Phase:** 29 — Symbol API Completion
**Depends on:** 363

## Problem

`Symbol.match`, `Symbol.replace`, `Symbol.search`, `Symbol.split`, `Symbol.matchAll` are well-known symbols used by RegExp and String methods. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-regexp-symbol/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const re = /\d+/g;
  const str = 'abc123def456';
  const match = str.match(re);
  const matchAll = Array.from(str.matchAll(re));

  return (
    <Box flexDirection="column">
      <Text>Match: {match?.join(', ') ?? 'none'}</Text>
      <Text>MatchAll count: {matchAll.length}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-symbol/`
- [ ] References `Symbol.match` / `Symbol.replace` / etc.
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
