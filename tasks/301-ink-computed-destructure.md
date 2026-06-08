# Task 301: `ink-computed-destructure` Example — Computed Keys and Renaming in Destructuring

**Priority:** P1-High
**Phase:** 25 — Advanced Destructuring Patterns
**Depends on:** 300

## Problem

Computed keys (`{ [key]: value }`) and renaming (`{ a: b }`) in destructuring allow dynamic property extraction. No existing Ink example exercises these patterns.

## Ink Example

```tsx
// examples/ink-computed-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const key = 'dynamicKey';
  const obj = { a: 1, b: 2, dynamicKey: 'found' };

  const { a: renamedA, [key]: dynamicValue } = obj;

  return (
    <Box flexDirection="column">
      <Text>Renamed A: {renamedA}</Text>
      <Text>Dynamic: {dynamicValue}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-computed-destructure/`
- [ ] Uses renaming (`{ a: b }`) in destructuring
- [ ] Uses computed key (`{ [expr]: value }`) in destructuring
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
