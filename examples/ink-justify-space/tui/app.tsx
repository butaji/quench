// Justify space example — exercises
import React from 'react';
// `justifyContent: "space-between"` and
// `justifyContent: "space-around"` to space
// children along the main axis.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function JustifySpace() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Box flexDirection="row" justifyContent="space-between" width={40}>
        <Text>Left</Text>
        <Text>Right</Text>
      </Box>
      <Box flexDirection="row" justifyContent="space-around" width={40}>
        <Text>A</Text>
        <Text>B</Text>
        <Text>C</Text>
      </Box>
      <Box flexDirection="row" justifyContent="space-between" width={40}>
        <Text>1</Text>
        <Text>2</Text>
        <Text>3</Text>
        <Text>4</Text>
      </Box>
    </Box>
  );
}
