// Cursor example — demonstrates cursor positioning.
// NOTE: useCursor hook is not yet supported in runts.
// This example shows cursor position (x: 0, y: 0) for parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Cursor() {
  // NOTE: useCursor is not supported in runts yet.
  // For parity testing, we show static position (0, 0).
  const x = 0;
  const y = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Cursor Example</Text>
      <Box marginTop={1}>
        <Text>
          <Text bold>Position:</Text> ({x}, {y})
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Cursor position displayed.</Text>
      </Box>
    </Box>
  );
}
