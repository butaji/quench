# Task 299: `ink-nullish-in-object` Example — Nullish Values in Object Literals

**Priority:** P2-Medium
**Phase:** 24 — Language Features
**Depends on:** 298

## Problem

Nullish values (`null`, `undefined`) in object literals exercise property serialization and JSON handling. No dedicated example exercises this edge case.

## Ink Example

```tsx
// examples/ink-nullish-in-object/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: null, b: undefined, c: 'present' };
  const keys = Object.keys(obj);
  const json = JSON.stringify(obj);

  return (
    <Box flexDirection="column">
      <Text>Keys: {keys.join(', ')}</Text>
      <Text>JSON: {json}</Text>
      <Text>Has b: {String('b' in obj)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-nullish-in-object/`
- [ ] Uses `null` and `undefined` as object literal values
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
