// Stdin example — exercises the useStdin hook.
// Demonstrates raw mode stdin access.
//
// This example shows how to check if raw mode is supported
// and the stdin interface.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useStdin } from 'ink';

export default function Stdin() {
  const { stdin, isRawModeSupported } = useStdin();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Stdin Hook</Text>
      <Box marginTop={1}>
        <Text>
          <Text bold>Raw mode supported:</Text>{' '}
          {isRawModeSupported ? 'Yes' : 'No'}
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Stdin is available for reading input.</Text>
      </Box>
    </Box>
  );
}
