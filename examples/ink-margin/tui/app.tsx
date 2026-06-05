// Margin example — exercises margin props on Box.
// Simplified for parity: uses Spacer elements for spacing
// to ensure consistent rendering across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer } from 'ink';

export default function Margin() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Margin Demo</Text>
      <Spacer />
      <Text>Header text</Text>
      <Spacer />
      <Box marginLeft={4}>
        <Text>Indented content</Text>
      </Box>
      <Spacer />
      <Text>Footer text</Text>
    </Box>
  );
}
