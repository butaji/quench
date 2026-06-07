# Task 057: `ink-generator` Example — `function*`, `yield`, `yield*`

**Priority:** P2-Medium  
**Phase:** 6 — Functions & Async  
**Depends on:** 056

## Problem

Zero examples use generators. HIR has `Expr::Yield` but parser falls through for generator functions.

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

Parser needs to handle `Function.generator = true` and create proper HIR for generator functions. Codegen needs to map generators to Rust iterators.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Generator functions parse into HIR (not skipped)
- [ ] `yield` and `yield*` produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
