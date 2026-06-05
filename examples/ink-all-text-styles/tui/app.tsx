// All text styles example — demonstrates all available text styling options.
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function AllTextStyles() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Text Styles Demo</Text>
      <Text></Text>
      <Text bold>Bold text</Text>
      <Text italic>Italic text</Text>
      <Text underline>Underline text</Text>
      <Text strikethrough>Strikethrough text</Text>
      <Text dimColor>Dim color text</Text>
      <Text inverse>Inverse text</Text>
      <Text></Text>
      <Text color="red">Red color</Text>
      <Text color="green">Green color</Text>
      <Text color="blue">Blue color</Text>
      <Text color="yellow">Yellow color</Text>
      <Text color="magenta">Magenta color</Text>
      <Text color="cyan">Cyan color</Text>
      <Text></Text>
      <Text backgroundColor="white" color="black">White background</Text>
    </Box>
  );
}
