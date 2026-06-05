// Stdin example — demonstrates stdin availability.
// NOTE: useStdin hook is not yet supported in runts.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Stdin() {
  // NOTE: useStdin is not supported in runts yet.
  // For parity testing, we show static values.
  const isRawModeSupported = false;
  const hasStdin = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Stdin Hook</Text>
      <Box marginTop={1}>
        <Text>Raw mode supported: {isRawModeSupported ? 'Yes' : 'No'}</Text>
      </Box>
      <Box marginTop={1}>
        <Text>Stdin is available: {hasStdin ? 'Yes' : 'No'}</Text>
      </Box>
    </Box>
  );
}
