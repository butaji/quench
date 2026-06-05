// Multiple colors example — demonstrates various text colors.
// Uses static text to ensure parity across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function MultipleColors() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Color Palette</Text>
      <Text color="black">black</Text>
      <Text color="red">red</Text>
      <Text color="green">green</Text>
      <Text color="yellow">yellow</Text>
      <Text color="blue">blue</Text>
      <Text color="magenta">magenta</Text>
      <Text color="cyan">cyan</Text>
      <Text color="white">white</Text>
    </Box>
  );
}
