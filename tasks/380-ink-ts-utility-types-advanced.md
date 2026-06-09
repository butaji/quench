# Task 380: `ink-ts-utility-types-advanced` Example — `Awaited`, `InstanceType`, `ConstructorParameters`, `ThisType`

**Priority:** P2-Medium
**Phase:** 31 — Advanced JSX + React Edge Cases
**Depends on:** 379

## Problem

TypeScript provides advanced built-in utility types (`Awaited<T>`, `InstanceType<T>`, `ConstructorParameters<T>`, `ThisType<T>`) that are useful for type-level programming. No existing Ink example explicitly exercises them.

## HIR Coverage

These types are erased during type erasure. The example validates that the parser correctly recognizes them as built-in type references and does not emit them into runtime HIR.

## Compile-Path Codegen

- No runtime codegen is required.
- The example must compile through `oxc_parser` → HIR → Rust codegen without emitting type references into runtime code.

## Ink Example

```tsx
// examples/ink-ts-utility-types-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Point {
  constructor(public x: number, public y: number) {}
}

type PointParams = ConstructorParameters<typeof Point>;
type PointInstance = InstanceType<typeof Point>;
type AsyncPoint = Promise<PointInstance>;
type Unwrapped = Awaited<AsyncPoint>;

const pt: Unwrapped = { x: 1, y: 2 };
const params: PointParams = [3, 4];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Point: ({pt.x}, {pt.y})</Text>
      <Text>Params: {params.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ts-utility-types-advanced/`
- [ ] Uses `Awaited`, `InstanceType`, `ConstructorParameters`, and `ThisType`
- [ ] Types are erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
