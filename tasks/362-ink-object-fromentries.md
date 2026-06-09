# Task 362: `ink-object-fromentries` Example — `Object.fromEntries`

**Priority:** P1-High
**Phase:** 29 — Object Methods Completion
**Depends on:** 361

## Problem

`Object.fromEntries` converts a list of key-value pairs into an object. Task 105 partially covers it; no dedicated example isolates this method.

## Ink Example

```tsx
// examples/ink-object-fromentries/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const entries: [string, number][] = [['a', 1], ['b', 2]];
  const obj = Object.fromEntries(entries);

  return (
    <Box flexDirection="column">
      <Text>Keys: {Object.keys(obj).join(', ')}</Text>
      <Text>A: {obj.a}</Text>
      <Text>B: {obj.b}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-fromentries/`
- [ ] Uses `Object.fromEntries`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
