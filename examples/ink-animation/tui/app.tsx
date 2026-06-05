// Animation example — demonstrates animated UI.
// Simplified version for cross-environment parity.
//
// NOTE: Animation/frame display is simplified here to avoid unicode issues.
// This example uses simple ASCII characters for parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

const FRAMES = ['*', '/', '-', '\\', '|'];

export default function Animation() {
  // Static frame for parity testing
  const frame = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text></Text>
      <Box borderStyle="round" padding={1}>
        <Text color="yellow" bold>
          [{FRAMES[frame]}] Loading...
        </Text>
      </Box>
      <Text dimColor marginTop={1}>
        Frame: {frame} of {FRAMES.length - 1}
      </Text>
    </Box>
  );
}
