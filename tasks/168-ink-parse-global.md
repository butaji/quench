# Task 168: `ink-parse-global` Example — `parseInt`, `parseFloat`, `isNaN`, `isFinite`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 167

## Problem

Global functions `parseInt`, `parseFloat`, `isNaN`, and `isFinite` are fundamental JavaScript globals for numeric parsing and validation. No existing Ink example explicitly exercises all four.

## Ink Example

```tsx
// examples/ink-parse-global/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const int1 = parseInt('42');
const int2 = parseInt('ff', 16);
const float1 = parseFloat('3.14');
const float2 = parseFloat('10.5px');

const nan1 = isNaN(NaN);
const nan2 = isNaN(42);
const fin1 = isFinite(100);
const fin2 = isFinite(Infinity);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>ParseInt: {int1}, {int2}</Text>
      <Text>ParseFloat: {float1}, {float2}</Text>
      <Text>IsNaN: {nan1 ? 'yes' : 'no'}, {nan2 ? 'yes' : 'no'}</Text>
      <Text>IsFinite: {fin1 ? 'yes' : 'no'}, {fin2 ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-parse-global/`
- [ ] Uses `parseInt`, `parseFloat`, `isNaN`, `isFinite`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
