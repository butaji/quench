# Task 115: `ink-this-parameter` Example — `this` Parameter, `this` Types

**Priority:** P2-Medium
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

`this` parameters in functions (`function fn(this: void)`) and `this` types in classes enable explicit typing of the `this` binding context. No existing Ink example exercises these.

## Ink Example

```tsx
// examples/ink-this-parameter/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Clickable {
  label: string;
  click(this: Clickable): string;
}

const button: Clickable = {
  label: 'Submit',
  click() {
    return `Clicked: ${this.label}`;
  },
};

function logThis(this: { prefix: string }, msg: string): string {
  return `${this.prefix}: ${msg}`;
}

class Counter {
  count = 0;
  increment(this: Counter): number {
    this.count++;
    return this.count;
  }
}

const counter = new Counter();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{button.click()}</Text>
      <Text>Count: {counter.increment()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-this-parameter/`
- [ ] Uses `this` parameter in function declaration
- [ ] Uses `this` type in class method
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `this` parameters without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
