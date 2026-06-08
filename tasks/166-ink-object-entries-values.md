# Task 166: `ink-object-entries-values` Example — `Object.entries`, `Object.values`, `Object.keys`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 105

## Problem

`Object.entries`, `Object.values`, and `Object.keys` are among the most commonly used JavaScript methods. No existing Ink example explicitly exercises all three together.

## Ink Example

```tsx
// examples/ink-object-entries-values/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const user = { name: 'Alice', age: 30, active: true };

const keys = Object.keys(user);
const values = Object.values(user);
const entries = Object.entries(user);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Keys: {keys.join(', ')}</Text>
      <Text>Values: {values.join(', ')}</Text>
      <Text>Entries: {entries.map(([k, v]) => `${k}=${v}`).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-entries-values/`
- [ ] Uses `Object.keys`, `Object.values`, `Object.entries`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
