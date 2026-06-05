// useApp hook example — demonstrates useApp for global app state.
// useApp returns the current exitFrame which can be used to exit the app.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-use-app
// 3. runts compile: runts build examples/ink-use-app --plugin ratatui --release

import React, { useState } from 'react';
import { Box, Text, useApp, useInput } from 'ink';

export default function UseAppExample() {
  const [count, setCount] = useState(0);
  const { exit } = useApp();

  useInput((input, key) => {
    if (input === 'q' || key.escape) {
      exit();
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useApp Hook Demo</Text>
      <Text></Text>
      <Text>Press up/down to change count.</Text>
      <Text>Press q or esc to exit.</Text>
      <Text></Text>
      <Text bold>Count: {count}</Text>
      <Box justifyContent="center">
        <Text
          backgroundColor="green"
          color="black"
          dimColor={count === 0}
        >
          {' '.repeat(Math.min(count, 20))}{count > 0 ? '●' : ''}
        </Text>
      </Box>
      <Text></Text>
      <Text dimColor>
        The useApp hook provides exit() to programmatically exit.
      </Text>
    </Box>
  );
}
