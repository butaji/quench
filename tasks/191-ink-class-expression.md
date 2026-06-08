# Task 191: `ink-class-expression` Example — Anonymous Class Expressions

**Priority:** P1-High
**Phase:** 17 — Expression-Level TypeScript Features
**Depends on:** 190

## Problem

Anonymous class expressions (`const MyClass = class { ... }`) are a core JavaScript feature that exercises HIR class handling without requiring a named declaration. No existing Ink example exercises anonymous class expressions.

## Ink Example

```tsx
// examples/ink-class-expression/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const Counter = class {
  count = 0;
  increment(): number {
    return ++this.count;
  }
};

const instance = new Counter();
instance.increment();
instance.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Count: {instance.count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-class-expression/`
- [ ] Uses anonymous class expression (`const X = class { ... }`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
