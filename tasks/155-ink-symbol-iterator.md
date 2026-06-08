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

- [x] Example exists at `examples/ink-symbol-iterator/`
- [x] Uses `Symbol.iterator` in class
- [x] Uses generator as iterator
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust
- [x] Parity harness passes with 100% match in all 3 environments

## Implementation Notes

Created comprehensive example demonstrating:
- Custom `Range` class with `[Symbol.iterator]` generator method
- Custom `PairList` class with iterable protocol
- Object with inline `[Symbol.iterator]`
- String and Array iteration (built-in iterables)
- Direct iterator protocol usage with `next()`
- Spread operator with custom iterables

Added `test_ink_symbol_iterator` test to `src/transpile/tests/rq_parity/mod.rs`.
