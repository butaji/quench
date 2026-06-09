# Task 342: `ink-render-props-generic` Example — Render Props with Generics

**Priority:** P2-Medium
**Phase:** 27 — React Patterns
**Depends on:** 341

## Problem

Render props with generics (`<DataProvider<T> render={data => ...} />`) pass typed data to a child render function. Task 153 covers basic render props; no example covers generic render props.

## Ink Example

```tsx
// examples/ink-render-props-generic/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface ProviderProps<T> {
  data: T;
  render: (item: T) => React.ReactNode;
}

function Provider<T>({ data, render }: ProviderProps<T>) {
  return <Box>{render(data)}</Box>;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Provider data={{ name: 'World' }} render={item => <Text>Hello {item.name}</Text>} />
    </Box>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-render-props-generic/`
- [ ] Uses render props with generic component
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases generics without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
