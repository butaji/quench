// Border color example — exercises `borderColor`
// Simplified version for cross-environment parity.
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
      <Text bold color="cyan">Border Color</Text>
      <Box borderStyle="round" borderColor="green" padding={1}>
        <Text color="green">Green border</Text>
      </Box>
      <Box borderStyle="round" borderColor="red" padding={1}>
        <Text color="red">Red border</Text>
      </Box>
    </Box>
  );
}
