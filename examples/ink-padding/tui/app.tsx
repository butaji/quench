// Padding example — demonstrates spacing in Box components.
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Padding() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Padding Demo</Text>
      <Text>padding=1 - all sides</Text>
      <Text>paddingX=2 - left/right only</Text>
      <Text>paddingY=1 - top/bottom only</Text>
      <Text>Combined padding styles</Text>
    </Box>
  );
}
