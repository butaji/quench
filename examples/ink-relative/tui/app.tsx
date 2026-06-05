// Relative positioning example — exercises position="relative" styling.
// NOTE: position="relative" with top/left/right/bottom is simplified.
// Static layout shown for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function RelativePosition() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Position Demo</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text>Normal positioned content</Text>
        <Text></Text>
        <Text dimColor>Relative positioning simplified</Text>
      </Box>
    </Box>
  );
}
