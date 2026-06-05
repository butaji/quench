// Z-index example — demonstrates flexbox layout.
// Shows basic Box component with flexDirection.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function ZIndexDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Layout Demo</Text>
      <Box>
        <Text>A</Text>
        <Text>B</Text>
        <Text>C</Text>
      </Box>
    </Box>
  );
}
