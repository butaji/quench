# Task 172: `ink-array-mutators` Example — `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 171

## Problem

Array mutator methods (`push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`) are fundamental JavaScript operations. No existing Ink example explicitly exercises all of them.

## Ink Example

```tsx
// examples/ink-array-mutators/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const arr1 = [1, 2, 3];
arr1.push(4);
const popped = arr1.pop();

const arr2 = ['a', 'b', 'c'];
const shifted = arr2.shift();
arr2.unshift('z');

const arr3 = [3, 1, 2];
arr3.sort((a, b) => a - b);
arr3.reverse();

const arr4 = [1, 2, 3, 4, 5];
arr4.splice(1, 2, 'a', 'b');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Popped: {popped}, arr: {arr1.join(', ')}</Text>
      <Text>Shifted: {shifted}, arr: {arr2.join(', ')}</Text>
      <Text>Sorted/reversed: {arr3.join(', ')}</Text>
      <Text>Spliced: {arr4.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-mutators/`
- [ ] Uses `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
