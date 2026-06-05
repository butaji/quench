// Border color example — demonstrates border styling
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function BorderColor() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Border Color Demo</Text>
      <Text color="green">Green border</Text>
      <Text color="red">Red border</Text>
    </Box>
  );
}
