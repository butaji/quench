# Task 043: `ink-while-dowhile` Example — `while`, `do-while`, Labeled Statements

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 042

## Problem

Zero examples use `while`, `do-while`, or labeled statements.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App() {
  const nums: number[] = [];
  let w = 1;
  while (w <= 3) {
    nums.push(w);
    w++;
  }

  const more: number[] = [];
  let d = 1;
  do {
    more.push(d);
    d++;
  } while (d <= 3);

  outer: for (let i = 0; i < 2; i++) {
    for (let j = 0; j < 2; j++) {
      if (i === 1 && j === 1) break outer;
    }
  }

  return (
    <Box flexDirection="column">
      <Text>While: {nums.join(', ')}</Text>
      <Text>Do-while: {more.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `while` and `do-while` produce compilable Rust
- [ ] Labeled `break` produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
