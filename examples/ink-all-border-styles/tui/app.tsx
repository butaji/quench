// All border styles example — demonstrates all available border styles.
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function AllBorderStyles() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Border Styles Demo</Text>
      <Box marginTop={1}>
        <Box borderStyle="single" padding={1}>
          <Text>Single border</Text>
        </Box>
      </Box>
      <Box marginTop={1}>
        <Box borderStyle="double" padding={1}>
          <Text>Double border</Text>
        </Box>
      </Box>
      <Box marginTop={1}>
        <Box borderStyle="round" padding={1}>
          <Text>Round border</Text>
        </Box>
      </Box>
      <Box marginTop={1}>
        <Box borderStyle="bold" padding={1}>
          <Text>Bold border</Text>
        </Box>
      </Box>
      <Box marginTop={1}>
        <Box borderStyle="classic" padding={1}>
          <Text>Classic border</Text>
        </Box>
      </Box>
    </Box>
  );
}
