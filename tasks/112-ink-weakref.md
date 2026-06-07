# Task 112: `ink-weakref` Example — `WeakRef`, `FinalizationRegistry`

**Priority:** P2-Medium
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

`WeakRef` and `FinalizationRegistry` (ES2021) enable weak references and cleanup callbacks. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-weakref/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

let ref: WeakRef<{ name: string }> | null = null;
let finalized = false;

const registry = new FinalizationRegistry<string>((heldValue) => {
  finalized = true;
});

function setup() {
  const obj = { name: 'temp' };
  ref = new WeakRef(obj);
  registry.register(obj, 'cleanup');
}

setup();
const alive = ref?.deref()?.name ?? 'collected';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Ref: {alive}</Text>
      <Text>Finalized: {finalized ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-weakref/`
- [ ] Uses `WeakRef` with `deref()`
- [ ] Uses `FinalizationRegistry` with callback
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
