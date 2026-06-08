# Task 339: `ink-context-reducer` Example — Context with `useReducer`

**Priority:** P1-High
**Phase:** 27 — React Patterns
**Depends on:** 338

## Problem

Combining React Context with `useReducer` creates global state management. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-context-reducer/tui/app.tsx
import React, { createContext, useContext, useReducer } from 'react';
import { Box, Text } from 'ink';

type Action = { type: 'inc' } | { type: 'dec' };

const StateContext = createContext(0);
const DispatchContext = createContext<React.Dispatch<Action>>(() => {});

function reducer(state: number, action: Action): number {
  switch (action.type) {
    case 'inc': return state + 1;
    case 'dec': return state - 1;
    default: return state;
  }
}

function Counter() {
  const count = useContext(StateContext);
  const dispatch = useContext(DispatchContext);
  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Text>Dispatch loaded</Text>
    </Box>
  );
}

export default function App() {
  const [count, dispatch] = useReducer(reducer, 0);
  return (
    <StateContext.Provider value={count}>
      <DispatchContext.Provider value={dispatch}>
        <Counter />
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-context-reducer/`
- [ ] Uses Context with `useReducer`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
