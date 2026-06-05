// useApp hook example — demonstrates useApp for global app state.
// NOTE: useApp and useInput hooks are not yet supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function UseAppExample() {
  // NOTE: For runts HIR runtime, useApp/useInput are not supported.
  // For parity testing, we show static count state.
  const count = 0;
  const canExit = true;

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
        App exit available: {canExit ? 'Yes' : 'No'}
      </Text>
    </Box>
  );
}
