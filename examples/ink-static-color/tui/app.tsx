// Static color example — pure Ink source that
import React from 'react';
// exercises color + backgroundColor on every
// Text variant. Useful for visual smoke tests
// of the runts-ink color palette.
//
// Renders a 3x3 grid of colored labels and
// background swatches.
//
// All three environments must produce the
// same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';

export default function StaticColor() {
  return (
    <Box flexDirection="column" borderStyle="bold" paddingX={1} paddingY={0}>
      <Text color="red" backgroundColor="black">red on black</Text>
      <Text color="green" backgroundColor="black">green on black</Text>
      <Text color="blue" backgroundColor="white">blue on white</Text>
      <Text color="yellow" backgroundColor="black">yellow on black</Text>
      <Text color="magenta" backgroundColor="black">magenta on black</Text>
      <Text color="cyan" backgroundColor="black">cyan on black</Text>
      <Text bold inverse>inverse bold</Text>
    </Box>
  );
}
