// Align self example — exercises `alignSelf`
// Simplified version for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function AlignSelf() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Align Self Demo</Text>
      <Text>flex-start</Text>
      <Text>center</Text>
      <Text>flex-end</Text>
    </Box>
  );
}
