// Padding example — exercises padding, paddingX, and paddingY props.
// Shows how to control internal spacing in Box components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Padding() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Padding Demo</Text>
      <Text></Text>
      
      <Box borderStyle="round" padding={1}>
        <Text>padding={1} - all sides</Text>
      </Box>
      
      <Box borderStyle="round" paddingX={2}>
        <Text>paddingX={2} - left/right only</Text>
      </Box>
      
      <Box borderStyle="round" paddingY={1}>
        <Text>paddingY={1} - top/bottom only</Text>
      </Box>
      
      <Box borderStyle="round" padding={1} paddingLeft={3}>
        <Text>Combined padding styles</Text>
      </Box>
    </Box>
  );
}
