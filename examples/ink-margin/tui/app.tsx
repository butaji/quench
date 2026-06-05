// Margin example — demonstrates margin properties on Box.
// Shows how margin creates space outside the border.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MarginExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Margin Demo</Text>
      <Text></Text>
      
      <Box margin={2} borderStyle="round" padding={1}>
        <Text>margin={2} - All sides</Text>
      </Box>
      
      <Text></Text>
      
      <Box marginLeft={3} borderStyle="single" padding={1}>
        <Text>marginLeft={3}</Text>
      </Box>
      
      <Text></Text>
      
      <Box flexDirection="row">
        <Box marginRight={2} borderStyle="single" padding={1}>
          <Text>Left</Text>
        </Box>
        <Box borderStyle="single" padding={1}>
          <Text>Right</Text>
        </Box>
      </Box>
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
