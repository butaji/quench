# Task 190: `ink-iterator-helpers` Example — Iterator Helpers (`map`, `filter`, `take`)

**Priority:** P2-Medium
**Phase:** 17 — Stage 3 TC39 Features
**Depends on:** 189

## Problem

Iterator helpers (`Iterator.from`, `.map()`, `.filter()`, `.take()`, `.drop()`, `.reduce()`) are a Stage 3 TC39 proposal for operating on iterators directly. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-iterator-helpers/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function* range(start: number, end: number) {
  for (let i = start; i <= end; i++) yield i;
}

export default function App() {
  const iter = range(1, 10);
  const mapped = Array.from(iter).map(n => n * 2);
  const filtered = mapped.filter(n => n > 10);
  const taken = filtered.slice(0, 3);

  return (
    <Box flexDirection="column">
      <Text>Result: {taken.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-iterator-helpers/`
- [ ] Uses iterator patterns with map/filter/take
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
