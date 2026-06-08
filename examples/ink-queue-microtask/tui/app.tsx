import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>queueMicrotask Demo</Text>
      <Text>Has queueMicrotask: {typeof queueMicrotask === 'function' ? 'yes' : 'no'}</Text>
    </Box>
  );
}
