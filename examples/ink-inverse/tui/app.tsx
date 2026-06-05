// Inverse example — demonstrates text styling options
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Inverse() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Inverse Demo</Text>
      <Text>Normal text</Text>
      <Text inverse>Inverse text</Text>
    </Box>
  );
}
