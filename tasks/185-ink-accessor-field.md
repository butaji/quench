# Task 185: `ink-accessor-field` Example — `accessor` Class Fields (TS 5.0)

**Priority:** P1-High
**Phase:** 17 — TypeScript 5.0+ Features
**Depends on:** 184

## Problem

`accessor` class fields (`accessor name: string`) are a TypeScript 5.0 feature for auto-generating getters and setters with private backing storage. No existing Ink example exercises them.

## Implementation Notes

The `accessor` keyword is:
- ✅ Supported by our oxc-based transpiler (via `transform_accessor_fields()` post-processing)
- ✅ Works in the rquickjs dev path (runts dev)
- ❌ Not supported by Deno's TypeScript parser (Deno 2.8.2 doesn't recognize the keyword)
- ❌ Not fully supported by compile path HIR codegen (class field initializers not captured in HIR)

## Ink Example

```tsx
// examples/ink-accessor-field/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  accessor value = 0;

  increment(): void {
    this.value++;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {counter.value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-accessor-field/`
- [x] Uses `accessor` class field
- [x] Renders identically in `runts dev` (rq path) with 100% output match
- [ ] Compile path: generates compilable Rust for accessor fields (blocked by HIR class field limitation)
- [x] Parity harness passes in rq environment

## Notes

The `accessor` keyword is transformed by `transform_accessor_fields()` in `src/transpile/postprocess.rs` which strips the keyword, treating it as a regular field for JS output.

Deno parity is skipped because Deno 2.x's TypeScript parser doesn't yet support the experimental `accessor` keyword. Compile path parity is skipped because the HIR doesn't capture class field initializers - the codegen generates structs without field values.

To make compile path work, would need:
1. Add `is_accessor` and `initializer` fields to `ClassMember` in HIR
2. Add `initializer: Option<Expr>` to capture class field initializers
3. Update parser to capture these
4. Update codegen to emit field initialization
