// Newline example — demonstrates the <Newline> component.
// Simplified for parity: uses simple layout with newline demonstrations.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Newline } from 'ink';

export default function NewlineExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Newline Demo</Text>
      <Box marginTop={1}>
        <Text>
          Line 1
          <Newline />
          Line 2
          <Newline />
          Line 3
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Use Newline to add breaks</Text>
      </Box>
    </Box>
  );
}
