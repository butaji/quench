// Absolute position example — exercises
import React from 'react';
// `position="absolute"` with `top`, `right`,
// `bottom`, `left` to position a Box at
// specific coordinates within its parent.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function Absolute() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text>Normal flow item 1</Text>
      <Box position="absolute" top={0} right={0}>
        <Text color="red">TOP-RIGHT</Text>
      </Box>
      <Text>Normal flow item 2</Text>
      <Box position="absolute" bottom={0} left={2}>
        <Text color="cyan">BOTTOM-LEFT</Text>
      </Box>
    </Box>
  );
}
