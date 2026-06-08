# Task 155: `ink-symbol-iterator` Example — `Symbol.iterator`, Custom Iterables

**Priority:** P1-High
**Phase:** 14 — Runtime API Completion
**Depends on:** 089

## Problem

`Symbol.iterator` and custom iterable objects are core JavaScript patterns for defining iteration behavior. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-symbol-iterator/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Range {
  constructor(private start: number, private end: number) {}

  *[Symbol.iterator]() {
    for (let i = this.start; i <= this.end; i++) {
      yield i;
    }
  }
}

const range = new Range(1, 5);
const nums = [...range];

const str = 'hello';
const chars = [...str];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Numbers: {nums.join(', ')}</Text>
      <Text>Chars: {chars.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-symbol-iterator/`
- [ ] Uses `Symbol.iterator` in class
- [ ] Uses generator as iterator
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
