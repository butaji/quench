# Task 083: `ink-access-modifiers` Example — `public`/`private`/`protected`/`readonly`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Class access modifiers (`public`, `private`, `protected`) and `readonly` properties are fundamental TypeScript features for encapsulation. No existing Ink example exercises these in a TUI context.

## Ink Example

```tsx
// examples/ink-access-modifiers/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  private count = 0;
  public readonly name: string;
  protected max: number;

  constructor(name: string, max: number) {
    this.name = name;
    this.max = max;
  }

  public increment(): void {
    if (this.count < this.max) {
      this.count++;
    }
  }

  public getValue(): number {
    return this.count;
  }
}

const counter = new Counter('Items', 5);
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {counter.name}</Text>
      <Text>Count: {counter.getValue()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-access-modifiers/`
- [ ] Uses `public`, `private`, `protected`, and `readonly` modifiers
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust (modifiers erased or mapped)
- [ ] Parity harness passes with 100% match in all 3 environments
