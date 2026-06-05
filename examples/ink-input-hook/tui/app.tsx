// Input hook example — demonstrates keyboard input handling.
// NOTE: useInput hook is not yet supported in runts HIR runtime.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function InputHook() {
  // NOTE: For runts HIR runtime, useInput is not supported.
  // For parity testing, we show static counter state.
  const count = 0;

  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Counter: {count}</Text>
      <Text dimColor>Press Enter to increment, q to quit</Text>
    </Box>
  );
}
