// Partial border example — demonstrates border styling
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function PartialBorder() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Partial Border Demo</Text>
      <Text>Top border only</Text>
      <Text>Bottom border only</Text>
      <Text>Left border only</Text>
      <Text>Right border only</Text>
    </Box>
  );
}
