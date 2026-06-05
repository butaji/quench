// Counter example — demonstrates useState and useInput hooks with Ink.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — interactive with keyboard
//   2. runts dev (HIR runtime) — static render with initial state
//   3. runts build (codegen->runts-ink) — full interactivity

import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';

export default function Counter() {
  const [count, setCount] = useState(0);

  useInput((input, key) => {
    if (input === 'q') {
      process.exit(0);
    }
    if (key.upArrow) {
      setCount(c => c + 1);
    }
    if (key.downArrow) {
      setCount(c => c - 1);
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Counter</Text>
      <Text></Text>
      <Text bold>Count: {count}</Text>
      <Text></Text>
      <Text italic dimColor>Press up/down to change count.</Text>
      <Text italic dimColor>Press q to quit.</Text>
    </Box>
  );
}
