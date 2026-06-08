# Task 229: `ink-atomics` Example — `Atomics` API

**Priority:** P3-Low
**Phase:** 20 — Runtime API Deep Coverage
**Depends on:** 228

## Problem

The `Atomics` API provides atomic operations on `SharedArrayBuffer`. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-atomics/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const buffer = new SharedArrayBuffer(4);
  const view = new Int32Array(buffer);

  Atomics.store(view, 0, 42);
  const loaded = Atomics.load(view, 0);
  const added = Atomics.add(view, 0, 8);
  const subbed = Atomics.sub(view, 0, 2);
  const exchanged = Atomics.exchange(view, 0, 100);

  return (
    <Box flexDirection="column">
      <Text>Loaded: {loaded}</Text>
      <Text>Added (old): {added}</Text>
      <Text>Subbed (old): {subbed}</Text>
      <Text>Exchanged (old): {exchanged}</Text>
      <Text>Current: {Atomics.load(view, 0)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-atomics/`
- [ ] Uses `Atomics.load`, `store`, `add`, `sub`, `exchange`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Atomics API
- [ ] Parity harness passes with 100% match in all 3 environments
