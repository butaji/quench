// Fragment example — demonstrates React fragment usage with Ink.
// Fragments allow grouping elements without adding extra nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function FragmentDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Fragment Demo</Text>
      <Text></Text>
      <Text>Using fragments to group elements:</Text>
      <Text></Text>
      {/* Fragment groups items without adding a Box */}
      <Box>
        <Text>Item 1</Text>
        <Text> | </Text>
        <Text>Item 2</Text>
        <Text> | </Text>
        <Text>Item 3</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Fragments help reduce nesting.</Text>
    </Box>
  );
}

export default FragmentDemo;
