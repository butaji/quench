import React, { useState } from 'react';
import { Box, Text } from 'ink';

const button = {
  label: 'Submit',
  click() {
    return `Clicked: ${this.label}`;
  },
};

export default function App() {
  const [count] = useState(0);
  return (
    <Box flexDirection="column">
      <Text>{button.click()}</Text>
      <Text>Count: {count}</Text>
    </Box>
  );
}
