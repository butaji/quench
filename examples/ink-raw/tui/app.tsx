// Raw mode example — demonstrates terminal raw mode detection.
// Simplified for parity: shows raw mode support status.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useStdin } from 'ink';

export default function RawMode() {
  const { isRawModeSupported } = useStdin();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Raw Mode Demo</Text>
      <Box marginTop={1}>
        <Text>Raw mode supported: </Text>
        <Text color={isRawModeSupported ? 'green' : 'red'}>
          {isRawModeSupported ? 'Yes' : 'No'}
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Terminal supports raw mode input</Text>
      </Box>
    </Box>
  );
}
