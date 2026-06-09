// ink-accessor-field example — demonstrates accessor fields (TypeScript 4.9+).

import React from 'react';
import { Box, Text } from 'ink';

class Counter {
  accessor value = 0;

  increment(): void {
    this.value++;
  }
}

const counter = new Counter();
counter.increment();
counter.increment();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {counter.value}</Text>
    </Box>
  );
}
