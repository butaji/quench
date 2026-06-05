import { Box, Text } from 'ink';
import React from 'react';

export default function App() {
  const isActive = true;
  const count = 3;
  const items = ['first', 'second', 'third'];
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round">
      <Text color={isActive ? 'green' : 'red'}>
        Status: {isActive ? 'ACTIVE' : 'INACTIVE'}
      </Text>
      <Text>Count: {count}</Text>
      <Text>Item 1: {items[0]}</Text>
      <Text>Item 2: {items[1]}</Text>
      <Text>Item 3: {items[2]}</Text>
    </Box>
  );
}
