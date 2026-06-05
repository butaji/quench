// Flex basis example — demonstrates width property for Box.
// Shows how elements can have fixed sizes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FlexBasisDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Width Demo</Text>
      <Box>
        <Box width={8} borderStyle="single">
          <Text dimColor>w:8</Text>
        </Box>
        <Box width={10} borderStyle="single">
          <Text dimColor>w:10</Text>
        </Box>
      </Box>
    </Box>
  );
}
