# Task 128: `ink-number-static` Example — `Number.isFinite`, `isNaN`, `parseInt`, `parseFloat`, `EPSILON`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 127

## Problem

`Number` static methods (`isFinite`, `isNaN`, `parseInt`, `parseFloat`) and constants (`EPSILON`, `MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`) are commonly used for numeric validation and parsing. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-number-static/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const values = ['42', '3.14', 'hello', 'Infinity', 'NaN'];

const parsed = values.map(v => ({
  input: v,
  isNum: Number.isFinite(Number(v)),
  isNaN: Number.isNaN(Number(v)),
  parsedInt: Number.parseInt(v, 10),
  parsedFloat: Number.parseFloat(v),
}));

export default function App() {
  return (
    <Box flexDirection="column">
      {parsed.map((p, i) => (
        <Text key={i}>{p.input}: finite={p.isNum ? 'yes' : 'no'}, NaN={p.isNaN ? 'yes' : 'no'}, int={p.parsedInt}, float={p.parsedFloat}</Text>
      ))}
      <Text>Epsilon: {Number.EPSILON}</Text>
      <Text>Max Safe: {Number.MAX_SAFE_INTEGER}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-number-static/`
- [ ] Uses `Number.isFinite`, `Number.isNaN`, `Number.parseInt`, `Number.parseFloat`
- [ ] Uses `Number.EPSILON`, `Number.MAX_SAFE_INTEGER`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
