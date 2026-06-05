// Overflow example — demonstrates overflow property control.
// Shows how content overflow is handled.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function OverflowExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Overflow Demo</Text>
      <Text></Text>
      
      <Box 
        width={30} 
        borderStyle="single" 
        padding={1}
        overflow="hidden"
      >
        <Text>This text will be clipped if it exceeds the box width.</Text>
      </Box>
      
      <Text></Text>
      
      <Box 
        width={30} 
        borderStyle="single" 
        padding={1}
        overflowX="hidden"
      >
        <Text>Horizontal overflow is hidden here.</Text>
      </Box>
      
      <Text></Text>
      
      <Box 
        height={3} 
        borderStyle="single" 
        padding={1}
        overflowY="hidden"
      >
        <Text>Line 1</Text>
        <Text>Line 2</Text>
        <Text>Line 3</Text>
        <Text>Line 4</Text>
        <Text>Line 5</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
