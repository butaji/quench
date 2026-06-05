// Gaps example — exercises gap, columnGap, rowGap props on Box.
// Demonstrates the CSS gap property equivalent for flexbox.
//
// The gap property adds space between flex children.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Gaps() {
  return (
    <Box flexDirection="column" padding={1} gap={1}>
      <Text bold color="cyan">Gap Property</Text>
      
      <Text>Row gap (column layout):</Text>
      <Box 
        flexDirection="column" 
        gap={2}
        borderStyle="round" 
        padding={1}
      >
        <Text>Item 1 (gap=2)</Text>
        <Text>Item 2</Text>
        <Text>Item 3</Text>
      </Box>
      
      <Text>Column gap (row layout):</Text>
      <Box 
        flexDirection="row" 
        columnGap={3}
        borderStyle="round" 
        padding={1}
      >
        <Text>A</Text>
        <Text>B</Text>
        <Text>C</Text>
      </Box>
      
      <Text>Combined row and column gap:</Text>
      <Box 
        flexDirection="column"
        gap={1}
        borderStyle="round" 
        padding={1}
      >
        <Box flexDirection="row" columnGap={2}>
          <Text>[1,1]</Text>
          <Text>[1,2]</Text>
          <Text>[1,3]</Text>
        </Box>
        <Box flexDirection="row" columnGap={2}>
          <Text>[2,1]</Text>
          <Text>[2,2]</Text>
          <Text>[2,3]</Text>
        </Box>
      </Box>
    </Box>
  );
}
