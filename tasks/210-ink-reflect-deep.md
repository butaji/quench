# Task 210: `ink-reflect-deep` Example — Reflect Methods (apply, construct, defineProperty, deleteProperty, etc.)

**Priority:** P2-Medium
**Phase:** 19 — Runtime API Deep Coverage
**Depends on:** 209

## Problem

Advanced `Reflect` methods (`apply`, `construct`, `defineProperty`, `deleteProperty`, `getOwnPropertyDescriptor`, `getPrototypeOf`, `setPrototypeOf`, `preventExtensions`, `isExtensible`) are not covered by any existing task. Task 096 covers `get`, `set`, `has`, `ownKeys` but not these.

## Ink Example

```tsx
// examples/ink-reflect-deep/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(a: string, b: string): string {
  return `${a} ${b}`;
}

class Point {
  constructor(public x: number, public y: number) {}
}

export default function App() {
  const applied = Reflect.apply(greet, null, ['Hello', 'World']);
  const constructed = Reflect.construct(Point, [1, 2]);
  const obj: any = {};
  Reflect.defineProperty(obj, 'value', { value: 42, writable: true });
  const descriptor = Reflect.getOwnPropertyDescriptor(obj, 'value');
  const proto = Reflect.getPrototypeOf(obj);
  const extensible = Reflect.isExtensible(obj);

  return (
    <Box flexDirection="column">
      <Text>Applied: {applied}</Text>
      <Text>Constructed: ({constructed.x}, {constructed.y})</Text>
      <Text>Defined: {obj.value}</Text>
      <Text>Descriptor: {descriptor ? 'yes' : 'no'}</Text>
      <Text>Extensible: {String(extensible)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-reflect-deep/`
- [ ] Uses `Reflect.apply`, `construct`, `defineProperty`, `getOwnPropertyDescriptor`, `getPrototypeOf`, `isExtensible`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
