// Dimensions example — exercises width, height on Box.
// Simplified for parity: uses compact layout without
// trailing whitespace differences between environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Dimensions() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dimensions</Text>
      <Box width={15} borderStyle="round" padding={1} marginTop={1}>
        <Text>15</Text>
      </Box>
      <Box width={20} borderStyle="round" padding={1} marginTop={1}>
        <Text>20</Text>
      </Box>
    </Box>
  );
}
