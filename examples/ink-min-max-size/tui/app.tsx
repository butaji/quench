// Min/Max size example — exercises minWidth, maxWidth,
// minHeight, maxHeight props on Box.
//
// Demonstrates how constraints affect box sizing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MinMaxSize() {
  return (
    <Box flexDirection="column" padding={1} gap={1}>
      <Text bold color="cyan">Min/Max Size</Text>
      
      <Box borderStyle="round" padding={1}>
        <Text>No constraints</Text>
      </Box>
      
      <Box 
        minWidth={20} 
        maxWidth={40}
        borderStyle="round" 
        padding={1}
      >
        <Text>minWidth=20, maxWidth=40</Text>
      </Box>
      
      <Box 
        minHeight={3}
        borderStyle="round" 
        padding={1}
      >
        <Text>minHeight=3</Text>
      </Box>
      
      <Box 
        width={15}
        maxWidth={15}
        borderStyle="round" 
        padding={1}
      >
        <Text>width=maxWidth=15</Text>
      </Box>
    </Box>
  );
}
