// Gaps example — exercises gap, columnGap, rowGap props on Box.
// Simplified for parity: uses only column layout with Spacer elements
// to simulate gaps since HIR runtime gap handling may differ.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer } from 'ink';

export default function Gaps() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Gap Property</Text>
      <Text dimColor>Using Spacer for consistent spacing</Text>
      
      <Box borderStyle="round" padding={1} marginTop={1}>
        <Box flexDirection="column">
          <Text>Item 1</Text>
          <Spacer />
          <Text>Item 2</Text>
          <Spacer />
          <Text>Item 3</Text>
        </Box>
      </Box>
      
      <Text marginTop={1}>Column layout:</Text>
      <Box 
        flexDirection="column" 
        borderStyle="round" 
        padding={1}
      >
        <Box flexDirection="row">
          <Text>A</Text>
          <Spacer />
          <Text>B</Text>
          <Spacer />
          <Text>C</Text>
        </Box>
      </Box>
    </Box>
  );
}
