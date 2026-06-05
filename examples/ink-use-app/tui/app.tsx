// useApp hook example — demonstrates useApp for global app state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, useApp, useInput } from 'ink';

export default function UseAppExample() {
  const [count, setCount] = useState(0);
  const app = useApp();
  const canExit = !!app?.exit;

  useInput((input, key) => {
    if (key.upArrow) setCount(c => c + 1);
    if (key.downArrow) setCount(c => c - 1);
    if (input === 'q' || input === 'Q') app?.exit?.();
  });

  const bar = ' '.repeat(Math.max(0, Math.min(count, 20))) + (count > 0 ? '●' : '');

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useApp Hook Demo</Text>
      <Text></Text>
      <Text>Press up/down to change count.</Text>
      <Text>Press q or esc to exit.</Text>
      <Text></Text>
      <Text bold>Count: {count}</Text>
      <Box justifyContent="center">
        <Text backgroundColor="green" color="black" dimColor={count === 0}>
          {bar}
        </Text>
      </Box>
      <Text></Text>
      <Text dimColor>
        App exit available: {canExit ? 'Yes' : 'No'}
      </Text>
    </Box>
  );
}
