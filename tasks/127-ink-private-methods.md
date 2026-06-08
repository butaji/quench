# Task 127: `ink-private-methods` Example — Private Methods `#method()`, `#field in obj`

**Priority:** P1-High
**Phase:** 12 — ES2022+ Language Features
**Depends on:** 126

## Problem

Private methods (`#method()`) and the `in` operator for private fields (`#field in obj`) are ES2022 features for true encapsulation. Task 071/056 cover private fields but not private methods or the `in` operator.

## Ink Example

```tsx
// examples/ink-private-methods/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  #count = 0;

  #validate(n: number): boolean {
    return n >= 0;
  }

  increment(): void {
    if (this.#validate(this.#count + 1)) {
      this.#count++;
    }
  }

  getValue(): number {
    return this.#count;
  }

  static hasCount(obj: unknown): boolean {
    return obj instanceof Counter && #count in obj;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Count: {counter.getValue()}</Text>
      <Text>Has count: {Counter.hasCount(counter) ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-private-methods/`
- [ ] Uses private method `#method()`
- [ ] Uses `in` operator with private field `#field in obj`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
