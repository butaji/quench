// React refs and debug example — exercises createRef and useDebugValue.
// Simplified for parity: uses class component with createRef
// and custom hook with useDebugValue.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React, { createRef, useDebugValue, useState, useCallback } from 'react';
import { Box, Text } from 'ink';

function useCounter() {
  const [count, setCount] = useState(0);
  useDebugValue(count > 10 ? 'high' : 'low');
  const increment = useCallback(() => setCount(c => c + 1), []);
  return { count, increment };
}

class Display extends React.Component<{ text: string }> {
  ref = createRef<{ textContent: string }>();
  render() {
    return <Text>{this.props.text}</Text>;
  }
}

export default function App() {
  const { count } = useCounter();

  return (
    <Box flexDirection="column">
      <Display text={`Count: ${count}`} />
      <Text>createRef + useDebugValue exercised</Text>
    </Box>
  );
}
