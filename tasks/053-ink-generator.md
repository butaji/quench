# Task 053: `ink-generator` Example — `function*`, `yield`, `yield*`

**Priority:** P2-Medium
**Phase:** 6 — Functions & Async
**Depends on:** 052

## Problem

Zero examples use generators. Parser handles `generator` flag but body is not processed.

## Example

```tsx
import { Box, Text } from 'ink';

function* range(start: number, end: number) {
  for (let i = start; i <= end; i++) {
    yield i;
  }
}

function* combined() {
  yield* range(1, 3);
  yield 4;
}

export default function App() {
  const nums = [...combined()];

  return (
    <Box>
      <Text>{nums.join(', ')}</Text>
    </Box>
  );
}
```

## Work

**Requires Task 077 (generator body parsing in HIR).**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Generator functions parse into HIR with complete bodies
- [ ] `yield` and `yield*` produce compilable Rust (mapped to iterators)
- [ ] `runts build --release` produces working binary with 100% output match
