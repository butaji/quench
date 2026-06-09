# Task 381: `ink-ts-global-var-augmentation` Example — `declare var` Global Augmentation

**Priority:** P2-Medium
**Phase:** 31 — Advanced JSX + React Edge Cases
**Depends on:** 380

## Problem

TypeScript supports augmenting the global scope via `declare var` inside a `declare global` block. This pattern is common for adding custom properties to `window` or declaring global variables. No existing Ink example explicitly exercises `declare var` augmentation.

## HIR Coverage

Global augmentations are type-level constructs. The example validates that the parser strips `declare global` blocks and `declare var` declarations without emitting them into runtime HIR.

## Compile-Path Codegen

- No runtime codegen is required.
- `declare global` and `declare var` must be completely erased during parser → HIR conversion.

## Ink Example

```tsx
// examples/ink-ts-global-var-augmentation/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

declare global {
  var __APP_VERSION__: string;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Version: {__APP_VERSION__}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ts-global-var-augmentation/`
- [ ] Uses `declare global` with `declare var`
- [ ] Augmentation is erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
