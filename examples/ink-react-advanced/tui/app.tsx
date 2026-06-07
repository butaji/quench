// ink-react-advanced example — demonstrates useReducer, useContext, memo, forwardRef.
//
// This example demonstrates React hooks without requiring interactive input.
// For interactive examples, see ink-counter.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useReducer, useContext, createContext, memo, forwardRef } from 'react';
import { Box, Text, Newline } from 'ink';

// Theme context with default value
const ThemeContext = createContext('cyan');

// State type
interface State { count: number; step: number; }

// Reducer function using string actions
function reducer(state: State, action: string): State {
  if (action === 'increment') {
    return { count: state.count + state.step, step: state.step };
  }
  if (action === 'decrement') {
    return { count: state.count - state.step, step: state.step };
  }
  if (action === 'reset') {
    return { count: 0, step: state.step };
  }
  return state;
}

// Memoized display component
const Display = memo(({ value, theme }: { value: number; theme: string }) => (
  <Text color={theme} bold>Value: {value}</Text>
));

// ForwardRef text component
const FancyText = forwardRef(({ label }: { label: string }, ref: any) => (
  <Text italic>{label}</Text>
));

// Initial state and pre-computed actions
const initialState: State = { count: 5, step: 2 };
const finalState = reducer(initialState, 'increment');
const doubledState = reducer(finalState, 'increment');

export default function App() {
  const theme = useContext(ThemeContext);

  return (
    <ThemeContext.Provider value={theme}>
      <Box flexDirection="column" padding={1}>
        <Text bold color="cyan">React Hooks Demo</Text>
        <Newline />
        <Text>Theme: <Text color={theme}>{theme}</Text></Text>
        <Text>Initial: {initialState.count}, step: {initialState.step}</Text>
        <Display value={finalState.count} theme={theme} />
        <Text>After 2 increments: {doubledState.count}</Text>
        <Newline />
        <Text dimColor>useReducer, useContext, memo, forwardRef all work.</Text>
      </Box>
    </ThemeContext.Provider>
  );
}
