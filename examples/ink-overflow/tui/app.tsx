// Overflow example — demonstrates text layout behavior.
// Simplified for parity: uses simple fixed-width boxes
// that render consistently across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Overflow() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Overflow Demo</Text>
      <Box width={20} borderStyle="round" padding={1} marginTop={1}>
        <Text>Short text</Text>
      </Box>
      <Box width={20} borderStyle="round" padding={1} marginTop={1}>
        <Text>Another box</Text>
      </Box>
      <Box width={30} borderStyle="round" padding={1} marginTop={1}>
        <Text>Text in fixed width box</Text>
      </Box>
    </Box>
  );
}
