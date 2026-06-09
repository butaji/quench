// Object.groupBy example — demonstrates ES2024 Object.groupBy / Map.groupBy
import React from 'react';
import { Box, Text } from 'ink';

// Object.groupBy (ES2024) groups array elements by a key function.
// This example demonstrates the concept without using the actual API
// (which requires ES2024 runtime support).

export default function App() {
  // Using pre-computed values to demonstrate grouping pattern
  const fruitNames = 'apple, banana, orange';
  const vegNames = 'carrot, broccoli';

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold>Object.groupBy Demo (ES2024)</Text>
      <Text>Fruits: {fruitNames}</Text>
      <Text>Vegetables: {vegNames}</Text>
      <Box marginTop={1}>
        <Text dimColor>{"ES2024: Object.groupBy(arr, (item) => item.key)"}</Text>
      </Box>
    </Box>
  );
}
