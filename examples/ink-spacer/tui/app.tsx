// Spacer example — exercises the Spacer component
// and the Newline component.
//
// Spacer fills remaining space in a flex container.
// Newline adds vertical spacing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text, Newline, Spacer } from 'ink';
import React from 'react';

export default function App() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Text>First line</Text>
      <Newline />
      <Text>Second line after newline</Text>
      <Spacer />
      <Box flexDirection="row" width={50}>
        <Text>Left</Text>
        <Box flexGrow={1}><Text> </Text></Box>
        <Text>Right</Text>
      </Box>
    </Box>
  );
}
