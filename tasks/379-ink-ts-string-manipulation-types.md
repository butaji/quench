# Task 379: `ink-ts-string-manipulation-types` Example — `Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`

**Priority:** P2-Medium
**Phase:** 31 — Advanced JSX + React Edge Cases
**Depends on:** 378

## Problem

TypeScript provides built-in string manipulation utility types (`Uppercase<T>`, `Lowercase<T>`, `Capitalize<T>`, `Uncapitalize<T>`) that operate purely at the type level. No existing Ink example explicitly exercises them.

## HIR Coverage

These types are erased during type erasure. The example validates that the parser correctly recognizes them as type references and does not emit them into runtime HIR.

## Compile-Path Codegen

- No runtime codegen is required.
- The example must compile through `oxc_parser` → HIR → Rust codegen without emitting type references into runtime code.

## Ink Example

```tsx
// examples/ink-ts-string-manipulation-types/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type EventName = 'click' | 'hover';
type UpperEvent = Uppercase<EventName>;
type LowerEvent = Lowercase<UpperEvent>;
type CapEvent = Capitalize<LowerEvent>;
type UncapEvent = Uncapitalize<CapEvent>;

const events: UncapEvent[] = ['click', 'hover'];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Upper: {(['CLICK', 'HOVER'] as UpperEvent[]).join(', ')}</Text>
      <Text>Lower: {events.join(', ')}</Text>
      <Text>Count: {events.length}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ts-string-manipulation-types/`
- [ ] Uses `Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`
- [ ] Types are erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
