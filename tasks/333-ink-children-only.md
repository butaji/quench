# Task 333: `ink-children-only` Example — `Children.only`

**Priority:** P2-Medium
**Phase:** 27 — React Children API
**Depends on:** 332

## Problem

`React.Children.only(children)` asserts that `children` is a single React element. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-children-only/tui/app.tsx
import React, { Children } from 'react';
import { Box, Text } from 'ink';

function SingleChild({ children }: { children: React.ReactNode }) {
  const only = Children.only(children);
  return <Box>{only}</Box>;
}

export default function App() {
  return (
    <SingleChild>
      <Text>Only child</Text>
    </SingleChild>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-children-only/`
- [ ] Uses `Children.only`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
