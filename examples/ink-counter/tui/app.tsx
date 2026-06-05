// Ink-style counter with useInput hook.
// Press up/down arrows to change count.
// Press q to quit.

import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';

export default function Counter() {
  const [count, setCount] = useState(0);

  useInput((input, key) => {
    if (key.upArrow) {
      setCount(count + 1);
    } else if (key.downArrow) {
      setCount(Math.max(0, count - 1));
    } else if (input === 'q' || key.escape) {
      process.exit(0);
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Counter</Text>
      <Text></Text>
      <Text bold>Count: {count}</Text>
      <Text></Text>
      <Text italic>Press up/down to change count.</Text>
      <Text italic>Press q to quit.</Text>
    </Box>
  );
}
