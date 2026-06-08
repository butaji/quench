# Task 123: `ink-object-meta` Example — `create`, `defineProperty`, `freeze`, `seal`, `assign`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 122

## Problem

Meta-object methods (`Object.create`, `Object.defineProperty`, `Object.freeze`, `Object.seal`, `Object.assign`) are essential for advanced object manipulation. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-object-meta/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const proto = { type: 'widget' };
const obj = Object.create(proto);
obj.name = 'Button';

Object.defineProperty(obj, 'id', {
  value: 42,
  writable: false,
  enumerable: true,
  configurable: false,
});

const frozen = Object.freeze({ x: 1 });
const sealed = Object.seal({ y: 2 });
const merged = Object.assign({}, { a: 1 }, { b: 2 });

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Proto type: {(obj as any).type}</Text>
      <Text>Name: {obj.name}</Text>
      <Text>ID: {(obj as any).id}</Text>
      <Text>Frozen x: {frozen.x}</Text>
      <Text>Merged keys: {Object.keys(merged).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-meta/`
- [ ] Uses `Object.create`, `Object.defineProperty`, `Object.freeze`, `Object.seal`, `Object.assign`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
