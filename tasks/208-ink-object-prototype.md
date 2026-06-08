# Task 208: `ink-object-prototype` Example — Object Prototype Methods

**Priority:** P2-Medium
**Phase:** 19 — Runtime API Deep Coverage
**Depends on:** 207

## Problem

Object prototype methods (`toString`, `valueOf`, `hasOwnProperty`, `propertyIsEnumerable`, `isPrototypeOf`) are core JavaScript APIs. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-object-prototype/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: 1 };
  const proto = Object.prototype;

  const toStr = obj.toString();
  const valOf = obj.valueOf();
  const hasOwn = obj.hasOwnProperty('a');
  const isEnum = obj.propertyIsEnumerable('a');
  const isProto = proto.isPrototypeOf(obj);

  return (
    <Box flexDirection="column">
      <Text>toString: {toStr}</Text>
      <Text>valueOf: {JSON.stringify(valOf)}</Text>
      <Text>hasOwnProperty: {String(hasOwn)}</Text>
      <Text>propertyIsEnumerable: {String(isEnum)}</Text>
      <Text>isPrototypeOf: {String(isProto)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-prototype/`
- [ ] Uses `toString`, `valueOf`, `hasOwnProperty`, `propertyIsEnumerable`, `isPrototypeOf`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
