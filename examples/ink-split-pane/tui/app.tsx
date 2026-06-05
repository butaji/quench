// Split pane example — demonstrates split pane UI.
// Uses row layout for side-by-side content.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function SplitPane() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Split Pane</Text>
      <Text></Text>
      <Box>
        <Text>A | B</Text>
      </Box>
      <Box>
        <Text>C | D</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
