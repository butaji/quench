# Task 385: `ink-typeof-class` Example — `typeof` on Class Constructors

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 384

## Problem

TypeScript's `typeof` operator can extract the constructor type of a class: `type C = typeof MyClass`. This exercises the type system's handling of class constructor signatures.

## HIR Coverage

- `typeof` in type contexts is erased during type erasure.
- The parser must not emit `Expr::Invalid` for `typeof` in type positions.

## Compile-Path Codegen

- No runtime codegen is required.
- `typeof` in type contexts is erased at parse time.

## Ink Example

```tsx
// examples/ink-typeof-class/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Point {
  constructor(public x: number, public y: number) {}
}

type PointConstructor = typeof Point;

const pt = new Point(1, 2);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Point: ({pt.x}, {pt.y})</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-typeof-class/`
- [ ] Uses `typeof` on a class constructor in a type alias
- [ ] Types are erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
