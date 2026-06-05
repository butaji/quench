// Newline example — demonstrates the <Newline> component.
// Uses Box with multiple Text children to show newline behavior.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function NewlineExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Newline Demo</Text>
      <Text>Line 1</Text>
      <Text>Line 2</Text>
      <Text>Line 3</Text>
      <Text></Text>
      <Text dimColor>Each Text is on its own line</Text>
    </Box>
  );
}
