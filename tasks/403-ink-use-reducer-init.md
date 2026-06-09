# Task 403: `ink-use-reducer-init` Example — useReducer with Initialization Function

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 402

## Problem

`useReducer` supports a third argument: an initialization function. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Expr::Call` for `useReducer` with three arguments.
- The third argument is a function called lazily to compute initial state.

## Compile-Path Codegen

- `js_bundle/react_shim.rs` must expose `useReducer` with three arguments.
- The React shim must call the init function on first render.

## Ink Example

```tsx
// examples/ink-use-reducer-init/tui/app.tsx
import React, { useReducer } from 'react';
import { Box, Text } from 'ink';

interface State {
  count: number;
}

type Action = { type: 'increment' } | { type: 'decrement' };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'increment': return { count: state.count + 1 };
    case 'decrement': return { count: state.count - 1 };
    default: return state;
  }
}

function init(initialCount: number): State {
  return { count: initialCount * 2 };
}

export default function App() {
  const [state, dispatch] = useReducer(reducer, 5, init);

  return (
    <Box flexDirection="column">
      <Text>Count: {state.count}</Text>
      <Text>Init was 5, doubled to {state.count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-reducer-init/`
- [ ] Uses `useReducer(reducer, initialArg, init)` with init function
- [ ] React shim supports three-argument useReducer
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
