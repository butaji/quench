// Flex shrink example — demonstrates flexShrink for compression behavior.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FlexShrinkDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">flexShrink Demo</Text>
      <Text>Container width: 30</Text>
      <Box width={30} borderStyle="single">
        <Box width={10} flexShrink={1}>
          <Text>A</Text>
        </Box>
        <Box width={10} flexShrink={2}>
          <Text>B</Text>
        </Box>
        <Box width={15} flexShrink={0}>
          <Text>C</Text>
        </Box>
      </Box>
      <Text dimColor>Box C should not shrink (flexShrink=0)</Text>
    </Box>
  );
}
