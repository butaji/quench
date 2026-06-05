// Animation example — demonstrates animated UI.
// Simplified version for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Animation() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text>Frame: 0 of 4</Text>
    </Box>
  );
}
