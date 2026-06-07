# Task 089: `ink-symbol-collections` Example — Symbol, Map, Set, WeakMap

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`Symbol`, `Map`, `Set`, and `WeakMap` are standard ES2015+ collection types. No existing Ink example exercises these primitives in a TUI context.

## Ink Example

```tsx
// examples/ink-symbol-collections/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const id = Symbol('app-id');
const map = new Map<string, number>();
map.set('a', 1);
map.set('b', 2);

const set = new Set<string>(['x', 'y', 'z']);

const registry = new WeakMap<object, string>();
const key = {};
registry.set(key, 'secret');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Map keys: {Array.from(map.keys()).join(', ')}</Text>
      <Text>Set size: {set.size}</Text>
      <Text>Has symbol: {typeof id === 'symbol' ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-symbol-collections/`
- [ ] Uses `Symbol`, `Map`, `Set`, and `WeakMap`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
