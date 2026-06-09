# Task 369: `ink-symbol-wellknown` Example — Well-Known Symbols (`Symbol.iterator`, `Symbol.asyncIterator`, `Symbol.toStringTag`, `Symbol.hasInstance`, `Symbol.toPrimitive`, `Symbol.species`)

**Priority:** P1-High
**Phase:** 29 — Symbol API Completion
**Depends on:** 368

## Problem

Well-known symbols beyond `Symbol.iterator` and `Symbol.asyncIterator` are not covered by existing tasks. `Symbol.toStringTag`, `Symbol.hasInstance`, `Symbol.toPrimitive`, `Symbol.species` control object behavior.

## Ink Example

```tsx
// examples/ink-symbol-wellknown/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Custom {
  [Symbol.toStringTag] = 'Custom';

  [Symbol.toPrimitive](hint: string): string | number {
    return hint === 'number' ? 42 : 'custom';
  }
}

const obj = new Custom();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>toString: {Object.prototype.toString.call(obj)}</Text>
      <Text>Primitive: {String(obj)}</Text>
      <Text>Number: {Number(obj)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-symbol-wellknown/`
- [ ] Uses `Symbol.toStringTag` and `Symbol.toPrimitive`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
