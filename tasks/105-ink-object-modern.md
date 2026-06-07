# Task 105: `ink-object-modern` Example — Modern Object Methods

**Priority:** P1-High
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

Modern object methods (`fromEntries`, `hasOwn`, `getOwnPropertyDescriptors`, `groupBy`) are essential ES2022+ features. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-object-modern/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const entries: [string, number][] = [['a', 1], ['b', 2]];
const obj = Object.fromEntries(entries);

const target = { x: 1, y: 2 };
const hasX = Object.hasOwn(target, 'x');
const hasZ = Object.hasOwn(target, 'z');

const desc = Object.getOwnPropertyDescriptors(target);

const items = [
  { type: 'fruit', name: 'apple' },
  { type: 'veg', name: 'carrot' },
  { type: 'fruit', name: 'banana' },
];

// Object.groupBy is ES2024 - use conditional for compatibility
const grouped = (Object as any).groupBy ? (Object as any).groupBy(items, (i: any) => i.type) : {};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>FromEntries keys: {Object.keys(obj).join(', ')}</Text>
      <Text>Has x: {hasX ? 'yes' : 'no'}</Text>
      <Text>Has z: {hasZ ? 'yes' : 'no'}</Text>
      <Text>Descriptors: {Object.keys(desc).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-modern/`
- [ ] Uses `Object.fromEntries`, `Object.hasOwn`, `Object.getOwnPropertyDescriptors`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for all object methods
- [ ] Parity harness passes with 100% match in all 3 environments
