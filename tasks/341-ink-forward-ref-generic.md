# Task 341: `ink-forward-ref-generic` Example — `forwardRef` with Generic Components

**Priority:** P2-Medium
**Phase:** 27 — React Type Patterns
**Depends on:** 340

## Problem

`forwardRef` with generic components (`forwardRef<T, P>((props, ref) => ...)`) is a common but complex TypeScript pattern. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-forward-ref-generic/tui/app.tsx
import React, { forwardRef, useRef, useImperativeHandle } from 'react';
import { Box, Text } from 'ink';

interface FancyRef {
  focus(): void;
}

const Fancy = forwardRef<FancyRef, { label: string }>(({ label }, ref) => {
  useImperativeHandle(ref, () => ({
    focus: () => {},
  }));
  return <Text>{label}</Text>;
});

export default function App() {
  const ref = useRef<FancyRef>(null);
  return (
    <Box flexDirection="column">
      <Fancy ref={ref} label="ForwardRef generic" />
    </Box>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-forward-ref-generic/`
- [ ] Uses `forwardRef` with generic component
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases generics without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
