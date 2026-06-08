# Task 130: `ink-react-refs-debug` Example — `createRef`, `useDebugValue`

**Priority:** P2-Medium
**Phase:** 12 — React API Completion
**Depends on:** 129

## Problem

`React.createRef` (class-based refs) and `useDebugValue` (React DevTools label for custom hooks) are React APIs not yet exercised. No existing Ink example covers them.

## Ink Example

```tsx
// examples/ink-react-refs-debug/tui/app.tsx
import React, { createRef, useDebugValue, useState, useCallback } from 'react';
import { Box, Text } from 'ink';

function useCounter() {
  const [count, setCount] = useState(0);
  useDebugValue(count > 10 ? 'high' : 'low');
  const increment = useCallback(() => setCount(c => c + 1), []);
  return { count, increment };
}

class Display extends React.Component<{ text: string }> {
  ref = createRef<HTMLDivElement>();
  render() {
    return <Text>{this.props.text}</Text>;
  }
}

export default function App() {
  const { count, increment } = useCounter();

  return (
    <Box flexDirection="column">
      <Display text={`Count: ${count}`} />
      <Text>createRef + useDebugValue exercised</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-refs-debug/`
- [ ] Uses `React.createRef` in class component
- [ ] Uses `useDebugValue` in custom hook
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
