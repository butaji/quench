// Background color example — exercises
// `backgroundColor` prop on Box.
//
// Demonstrates different background colors including
// named colors and hex values.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function BackgroundColor() {
  return (
    <Box flexDirection="column" gap={1} padding={1}>
      <Box backgroundColor="black" padding={1}>
        <Text backgroundColor="black" color="white">Black bg, white text</Text>
      </Box>
      <Box backgroundColor="red" padding={1}>
        <Text backgroundColor="red" color="white">Red bg, white text</Text>
      </Box>
      <Box backgroundColor="green" padding={1}>
        <Text backgroundColor="green" color="white">Green bg, white text</Text>
      </Box>
      <Box backgroundColor="blue" padding={1}>
        <Text backgroundColor="blue" color="white">Blue bg, white text</Text>
      </Box>
      <Box backgroundColor="#ffcc00" padding={1}>
        <Text backgroundColor="#ffcc00" color="black">Yellow hex bg</Text>
      </Box>
    </Box>
  );
}
