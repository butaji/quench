// useId and useTransition example — React 18 hooks for accessibility and transitions
//
// useId: Generates a unique ID for accessibility attributes
// useTransition: Marks state updates as non-blocking transitions

import React, { useId, useState, useTransition } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [isPending, startTransition] = useTransition();
  const [count, setCount] = useState(0);
  const id = useId();

  const increment = () => {
    startTransition(() => {
      setCount(c => c + 1);
    });
  };

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>React 18 Hooks Demo</Text>
      <Text>ID: {id}</Text>
      <Text>Count: {count}</Text>
      <Text>Pending: {isPending ? 'yes' : 'no'}</Text>
      <Text dimColor>(useId generates stable IDs)</Text>
      <Text dimColor>(useTransition marks updates as non-blocking)</Text>
    </Box>
  );
}
