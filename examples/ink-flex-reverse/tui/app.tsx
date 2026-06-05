// Flex direction example — exercises flexDirection prop.
// Simplified for parity: uses column layout with
// simple spacing for consistent rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer } from 'ink';

export default function FlexReverse() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Flex Direction</Text>
      <Spacer />
      <Text>Column layout:</Text>
      <Box flexDirection="column" borderStyle="round" padding={1} marginTop={1}>
        <Text>First</Text>
        <Spacer />
        <Text>Second</Text>
        <Spacer />
        <Text>Third</Text>
      </Box>
      <Spacer />
      <Text>Row layout:</Text>
      <Box flexDirection="row" borderStyle="round" padding={1} marginTop={1}>
        <Text>A</Text>
        <Spacer />
        <Text>B</Text>
        <Spacer />
        <Text>C</Text>
      </Box>
    </Box>
  );
}
