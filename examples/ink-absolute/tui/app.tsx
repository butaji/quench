// Absolute position example — simplified for cross-environment parity.
// NOTE: Complex absolute positioning may not work in all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Absolute() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Position Demo</Text>
      <Box borderStyle="round" padding={1}>
        <Text>Item 1 at top</Text>
        <Text>Item 2 in middle</Text>
        <Text color="green">Item 3 at bottom</Text>
      </Box>
    </Box>
  );
}
