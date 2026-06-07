# Task 060: `ink-react-advanced` Example — `useReducer`, `useContext`, `memo`, `forwardRef`

**Priority:** P2-Medium  
**Phase:** 6 — React Advanced  
**Depends on:** 059

## Problem

91 examples only exercise basic hooks (`useState`, `useEffect`, `useRef`). Advanced hooks are not validated.

## Example

```tsx
import React, { useReducer, useContext, createContext, memo, forwardRef } from 'react';
import { Box, Text, useInput } from 'ink';

const ThemeContext = createContext('green');

interface State { count: number; step: number; }
type Action = { type: 'increment' } | { type: 'decrement' };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'increment': return { ...state, count: state.count + state.step };
    case 'decrement': return { ...state, count: state.count - state.step };
    default: return state;
  }
}

const Display = memo(({ value }: { value: number }) => (
  <Text>Value: {value}</Text>
));

const FancyText = forwardRef(({ label }: { label: string }, ref: any) => (
  <Text ref={ref}>{label}</Text>
));

export default function App() {
  const [state, dispatch] = useReducer(reducer, { count: 0, step: 1 });
  const theme = useContext(ThemeContext);

  useInput((input) => {
    if (input === 'q') process.exit(0);
    if (input === '+') dispatch({ type: 'increment' });
    if (input === '-') dispatch({ type: 'decrement' });
  });

  return (
    <ThemeContext.Provider value={theme}>
      <Box flexDirection="column">
        <Text color={theme}>useReducer Demo</Text>
        <Display value={state.count} />
        <FancyText label="Forwarded" />
      </Box>
    </ThemeContext.Provider>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `useReducer`, `useContext`, `memo`, `forwardRef` all work in rquickjs
- [ ] Compile path supports all hooks used
- [ ] `runts build --release` produces working binary with 100% output match
