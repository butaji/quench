# Task 111: `ink-proxy` Example — `Proxy` Handler

**Priority:** P2-Medium
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

`Proxy` is a powerful ES2015 meta-programming feature for intercepting object operations. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-proxy/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

const createLoggedState = <T extends object>(initial: T): T => {
  const handler: ProxyHandler<T> = {
    get(target, prop) {
      return Reflect.get(target, prop);
    },
    set(target, prop, value) {
      return Reflect.set(target, prop, value);
    },
  };
  return new Proxy(initial, handler);
};

const state = createLoggedState({ count: 0, name: 'App' });
state.count = 5;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Count: {state.count}</Text>
      <Text>Name: {state.name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-proxy/`
- [ ] Uses `Proxy` with `get` and `set` traps
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Proxy
- [ ] Parity harness passes with 100% match in all 3 environments
