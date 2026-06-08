# Task 296: `ink-computed-class-members` Example — Computed Property Keys in Classes

**Priority:** P1-High
**Phase:** 24 — Class Features
**Depends on:** 295

## Problem

Computed property keys in classes (`[Symbol.iterator]() {}`, `['computed' + 'Method']() {}`) allow dynamic member names. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-computed-class-members/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  [Symbol.toStringTag] = 'Counter';

  private count = 0;

  increment(): number {
    return ++this.count;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Tag: {counter[Symbol.toStringTag]}</Text>
      <Text>Count: {counter.increment()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-computed-class-members/`
- [ ] Uses computed property key with `Symbol.toStringTag`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for computed class members
- [ ] Parity harness passes with 100% match in all 3 environments
