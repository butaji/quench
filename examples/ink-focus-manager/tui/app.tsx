// Focus manager example — demonstrates programmatic focus control.
// Simplified version for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusManager() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Manager Demo</Text>
      <Text>Tab to move focus forward.</Text>
    </Box>
  );
}
