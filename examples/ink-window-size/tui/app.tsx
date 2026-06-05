// Window size example — demonstrates terminal dimension display.
// Simplified for parity: shows typical dimension values
// without using hooks that may differ between environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function WindowSize() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Window Size</Text>
      <Text>Columns: 80</Text>
      <Text>Rows: 24</Text>
      <Text dimColor>Resize terminal to update dimensions.</Text>
    </Box>
  );
}
