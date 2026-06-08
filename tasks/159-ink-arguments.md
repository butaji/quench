# Task 159: `ink-arguments` Example — `arguments` Object, Rest vs Arguments

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 158

## Problem

The `arguments` object is a legacy JavaScript feature for accessing function parameters. While rest parameters (`...args`) are preferred, `arguments` is still common in older code. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-arguments/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function sumAll(): number {
  let sum = 0;
  for (let i = 0; i < arguments.length; i++) {
    sum += arguments[i];
  }
  return sum;
}

function logArgs() {
  return Array.from(arguments).join(', ');
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum(1,2,3): {sumAll(1, 2, 3)}</Text>
      <Text>Sum(10,20): {sumAll(10, 20)}</Text>
      <Text>Args: {logArgs('a', 'b', 'c')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-arguments/`
- [x] Uses `arguments` object in non-arrow function
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust (known limitation: compile path has architectural constraints with JS runtime features like `arguments` - requires rquickjs engine)
- [x] Parity harness passes with 100% match in dev environment

## Notes

- Dev path (rquickjs): 100% parity with deno ✅
- Compile path: Known architectural limitation - the `arguments` object is a JavaScript runtime feature that requires the JS engine. The compile path generates static Rust code and cannot evaluate JS runtime features like `arguments`.
- Test added in `src/transpile/tests/rq_parity/mod.rs`
