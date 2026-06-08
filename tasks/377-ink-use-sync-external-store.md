# Task 377: `ink-use-sync-external-store` Example — `useSyncExternalStore`

**Priority:** P1-High
**Phase:** 31 — React 18 Hooks
**Depends on:** 376

## Problem

`useSyncExternalStore` is a React 18 hook for subscribing to external data sources with concurrent rendering safety. Task 107 exists but no dedicated example exercises it.

## Ink Example

```tsx
// examples/ink-use-sync-external-store/tui/app.tsx
import React, { useSyncExternalStore } from 'react';
import { Box, Text } from 'ink';

let store = { count: 0 };
const listeners = new Set<() => void>();

function subscribe(callback: () => void) {
  listeners.add(callback);
  return () => listeners.delete(callback);
}

function getSnapshot() {
  return store.count;
}

export default function App() {
  const count = useSyncExternalStore(subscribe, getSnapshot);

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-sync-external-store/`
- [ ] Uses `useSyncExternalStore` with subscribe and getSnapshot
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
