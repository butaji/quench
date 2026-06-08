# Task 223: `ink-private-in` Example — `#field in obj` (Private Identifier in `in`)

**Priority:** P1-High
**Phase:** 20 — Advanced Language Features
**Depends on:** 222

## Problem

The `in` operator with private identifiers (`#field in obj`) checks if an object has a private field. This is an ES2022 feature. Task 127 covers private methods but not the `in` operator for private fields.

## Ink Example

```tsx
// examples/ink-private-in/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  #count = 0;

  hasCount(): boolean {
    return #count in this;
  }

  increment(): void {
    this.#count++;
  }

  getCount(): number {
    return this.#count;
  }
}

const counter = new Counter();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Has #count: {String(counter.hasCount())}</Text>
      <Text>Count: {counter.getCount()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-private-in/`
- [ ] Uses `#field in obj` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for private-in operator
- [ ] Parity harness passes with 100% match in all 3 environments
