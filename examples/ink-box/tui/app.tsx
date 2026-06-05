// Box component example — demonstrates all Box layout properties.
// Box is the primary layout component in Ink, based on CSS flexbox.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function BoxDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Box Component Demo</Text>
      <Text></Text>
      
      {/* Basic column layout */}
      <Box flexDirection="column" borderStyle="round" padding={1}>
        <Text bold>Column Layout</Text>
        <Text>Item 1</Text>
        <Text>Item 2</Text>
        <Text>Item 3</Text>
      </Box>
      
      <Text></Text>
      
      {/* Row layout */}
      <Box borderStyle="single" padding={1}>
        <Text bold>Row: </Text>
        <Text>A</Text>
        <Text> B</Text>
        <Text> C</Text>
      </Box>
      
      <Text></Text>
      
      {/* Nested boxes */}
      <Box borderStyle="double" padding={1}>
        <Text bold>Nested Layout</Text>
        <Box flexDirection="row" gap={2} marginTop={1}>
          <Box borderStyle="round" padding={1}>
            <Text dimColor>Left</Text>
          </Box>
          <Box borderStyle="round" padding={1}>
            <Text dimColor>Right</Text>
          </Box>
        </Box>
      </Box>
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
