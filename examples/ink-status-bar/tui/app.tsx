// Status bar example — demonstrates status bar UI.
// Uses row layout with space-between for left/right content.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function StatusBar() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Application Title</Text>
      <Text></Text>
      
      {/* Status bar */}
      <Box 
        justifyContent="space-between"
        borderStyle="round"
        padding={1}
      >
        <Text dimColor>Ready</Text>
        <Text dimColor>v1.0.0</Text>
      </Box>
      
      <Text></Text>
      
      {/* Info bar */}
      <Box borderStyle="single" padding={1}>
        <Text>Left</Text>
        <Text>Center</Text>
        <Text>Right</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
