// Form layout example — demonstrates form-style layouts with labels and inputs.
// This pattern is common for CLI tools.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FormLayout() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">User Profile</Text>
      <Text></Text>
      <Box>
        <Box width={12}>
          <Text bold>Name:</Text>
        </Box>
        <Text>John Doe</Text>
      </Box>
      <Box>
        <Box width={12}>
          <Text bold>Email:</Text>
        </Box>
        <Text>john@example.com</Text>
      </Box>
      <Box>
        <Box width={12}>
          <Text bold>Role:</Text>
        </Box>
        <Text>Administrator</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
