# Task 177: `ink-void-comma-increment` Example — `void`, Comma Operator, `++`/`--`

**Priority:** P1-High
**Phase:** 16 — Operator Completion
**Depends on:** 176

## Problem

`void` operator, comma operator, and increment/decrement (`++`/`--`) are JavaScript operators not explicitly covered by any Ink example.

## Ink Example

```tsx
// examples/ink-void-comma-increment/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

let count = 0;
const voidResult = void (count = 10);
const commaResult = (count++, count++, count);

const pre = ++count;
const post = count++;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Void: {String(voidResult)}</Text>
      <Text>Comma: {commaResult}</Text>
      <Text>Pre: {pre}</Text>
      <Text>Post: {post}</Text>
      <Text>Final: {count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-void-comma-increment/`
- [ ] Uses `void` operator
- [ ] Uses comma operator
- [ ] Uses `++` and `--` (pre and post)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
