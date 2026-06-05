// Stdout example — exercises the useStdout hook.
// Demonstrates direct stdout access.
//
// This example shows how to access the stdout interface
// for writing directly to the terminal.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useStdout } from 'ink';

export default function Stdout() {
  const { write } = useStdout();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Stdout Hook</Text>
      <Box marginTop={1}>
        <Text dimColor>
          Stdout is available for direct terminal writing.
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text>
          Output can be written using the write() method.
        </Text>
      </Box>
    </Box>
  );
}
