# Task 205: `ink-set-prototype` Example — `Set`/`Map`/`WeakMap`/`WeakSet` Prototype Methods

**Priority:** P2-Medium
**Phase:** 17 — Runtime API Deep Coverage
**Depends on:** 204

## Problem

`Set`, `Map`, `WeakMap`, and `WeakSet` prototype methods (`add`, `delete`, `has`, `clear`, `size`, `get`, `set`, `keys`, `values`, `entries`, `forEach`) are core data structure APIs. Task 089 covers `Symbol` and collections but not the full prototype method surface. No existing Ink example exercises all collection prototype methods.

## Ink Example

```tsx
// examples/ink-set-prototype/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const set = new Set([1, 2, 3]);
  set.add(4);
  set.delete(2);

  const map = new Map<string, number>();
  map.set('a', 1);
  map.set('b', 2);
  const hasA = map.has('a');

  const wm = new WeakMap<object, string>();
  const key = {};
  wm.set(key, 'value');
  const wmHas = wm.has(key);

  const ws = new WeakSet<object>();
  const wsKey = {};
  ws.add(wsKey);
  const wsHas = ws.has(wsKey);

  return (
    <Box flexDirection="column">
      <Text>Set: {Array.from(set).join(', ')}</Text>
      <Text>Map A: {map.get('a')}</Text>
      <Text>Has A: {String(hasA)}</Text>
      <Text>WeakMap Has: {String(wmHas)}</Text>
      <Text>WeakSet Has: {String(wsHas)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-set-prototype/`
- [ ] Uses `Set.add`, `delete`, `has`, `size`
- [ ] Uses `Map.set`, `get`, `has`
- [ ] Uses `WeakMap.set`, `has`
- [ ] Uses `WeakSet.add`, `has`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
