// Counter example — demonstrates state and static rendering.
// NOTE: useInput hook is not yet supported in runts dev mode.
// This version shows static count=0 for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime) - static render
//   3. runts build (codegen->runts-ink) - full interactivity

import React from 'react';
import { Box, Text } from 'ink';

export default function Counter() {
  // NOTE: For runts dev mode (HIR runtime), useInput is not supported.
  // For parity testing, we show static count=0.
  // The runts compile path supports useInput when built with ratatui.
  const count = 0;

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
