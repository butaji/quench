// Fragment example — demonstrates React fragment usage with Ink.
// Fragments allow grouping elements without adding extra nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Fragment syntax and && operator are not supported in runts HIR runtime.
// For compatibility, we use Box to group elements.

import React from 'react';
import { Box, Text } from 'ink';

export default function FragmentDemo() {
  const items = ['Alpha', 'Beta', 'Gamma'];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Fragment Demo</Text>
      <Text></Text>
      
      <Text>Using fragments to group elements:</Text>
      <Text></Text>
      
      {/* Using Box to group items */}
      <Box>
        <Text color="green">{items[0]}</Text>
        <Text dimColor> | </Text>
        <Text color="green">{items[1]}</Text>
        <Text dimColor> | </Text>
        <Text color="green">{items[2]}</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Fragments help reduce nesting depth.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
