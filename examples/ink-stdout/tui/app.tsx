// Stdout example — demonstrates stdout availability.
// NOTE: useStdout hook is not yet supported in runts.
// Shows static values for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Stdout() {
  // NOTE: useStdout is not supported in runts yet.
  // For parity testing, we show static values.
  const hasStdout = true;
  const canWrite = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Stdout Hook</Text>
      <Box marginTop={1}>
        <Text>Stdout is available: {hasStdout ? 'Yes' : 'No'}</Text>
      </Box>
      <Box marginTop={1}>
        <Text>Direct writing supported: {canWrite ? 'Yes' : 'No'}</Text>
      </Box>
    </Box>
  );
}
