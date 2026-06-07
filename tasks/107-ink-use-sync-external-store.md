# Task 107: `ink-use-sync-external-store` Example — `useSyncExternalStore`, `useDeferredValue`

**Priority:** P1-High
**Phase:** 11 — React 18 Hook Coverage
**Depends on:** 078

## Problem

`useSyncExternalStore` (React 18, for external state) and `useDeferredValue` (React 18, for non-urgent updates) are important concurrent React hooks. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-use-sync-external-store/tui/app.tsx
import React, { useSyncExternalStore, useDeferredValue, useState } from 'react';
import { Box, Text } from 'ink';

let width = 80;
const listeners = new Set<() => void>();

const store = {
  subscribe: (cb: () => void) => {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },
  getSnapshot: () => width,
  setWidth: (w: number) => {
    width = w;
    listeners.forEach(cb => cb());
  },
};

export default function App() {
  const termWidth = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    () => 80
  );

  const [text, setText] = useState('hello');
  const deferredText = useDeferredValue(text);

  return (
    <Box flexDirection="column">
      <Text>Width: {termWidth}</Text>
      <Text>Deferred: {deferredText}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-sync-external-store/`
- [ ] Uses `useSyncExternalStore` with external store
- [ ] Uses `useDeferredValue` for non-urgent updates
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
