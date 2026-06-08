import React from 'react';
import { Box, Text } from 'ink';

// WeakRef example - weak reference to an object
const obj = { name: 'temp' };
const ref = new WeakRef(obj);
const alive = ref.deref()?.name ?? 'collected';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Ref: {alive}</Text>
      <Text>Finalized: pending</Text>
    </Box>
  );
}
