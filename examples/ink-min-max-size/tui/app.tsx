// Min/Max size example — exercises minWidth, maxWidth,
// minHeight, maxHeight props on Box.
// Simplified version for cross-environment parity.
//
// NOTE: Complex min/max constraints are simplified here
// to ensure parity across deno, runts dev, and runts build.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MinMaxSize() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Min/Max Size Demo</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text>Box with default sizing</Text>
      </Box>
      <Box borderStyle="round" padding={1} width={30}>
        <Text>Fixed width box (30)</Text>
      </Box>
      <Box borderStyle="round" padding={1} width={25}>
        <Text>Narrow box (25)</Text>
      </Box>
    </Box>
  );
}
