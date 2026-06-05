// Text styling example — demonstrates all Ink Text styling options:
// bold, italic, underline, strikethrough, dim, inverse, and color combinations.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-text-styling
// 3. runts compile: runts build examples/ink-text-styling --plugin ratatui --release

import React from 'react';
import { Box, Text } from 'ink';

export default function TextStyling() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Text Styling Demo</Text>
      <Text></Text>
      
      <Text bold>Bold Text</Text>
      <Text italic>Italic Text</Text>
      <Text underline>Underlined Text</Text>
      <Text strikethrough>Strikethrough Text</Text>
      <Text dimColor>Dim Text</Text>
      <Text inverse>Inverse Text</Text>
      
      <Text></Text>
      <Text bold color="red">Red Bold Text</Text>
      <Text italic color="green">Green Italic Text</Text>
      <Text underline color="blue">Blue Underlined Text</Text>
      
      <Text></Text>
      <Text bold italic underline>
        Multiple Styles Combined
      </Text>
      
      <Text></Text>
      <Text italic dimColor>Dim Italic Text</Text>
      <Text bold strikethrough color="yellow">Yellow Bold Strikethrough</Text>
    </Box>
  );
}
