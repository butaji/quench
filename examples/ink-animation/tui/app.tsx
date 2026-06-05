// Animation example — demonstrates animated UI.
// Simplified version for cross-environment parity.
// Uses simple ASCII characters to avoid unicode rendering issues.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Animation() {
  // Static frame for parity testing (avoids runtime animation differences)
  const frame = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text color="yellow" bold>
          [*] Loading...
        </Text>
      </Box>
      <Text dimColor marginTop={1}>
        Frame: {frame} of 4
      </Text>
    </Box>
  );
}
