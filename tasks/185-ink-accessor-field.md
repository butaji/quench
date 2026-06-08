# Task 185: `ink-accessor-field` Example — `accessor` Class Fields (TS 5.0)

**Priority:** P1-High
**Phase:** 17 — TypeScript 5.0+ Features
**Depends on:** 184

## Problem

`accessor` class fields (`accessor name: string`) are a TypeScript 5.0 feature for auto-generating getters and setters with private backing storage. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-accessor-field/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  accessor value = 0;

  increment(): void {
    this.value++;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {counter.value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-accessor-field/`
- [ ] Uses `accessor` class field
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for accessor fields
- [ ] Parity harness passes with 100% match in all 3 environments
