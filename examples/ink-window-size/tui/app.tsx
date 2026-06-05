// Window size example — exercises the useWindowSize hook.
// Displays the current terminal dimensions.
//
// The display updates when the terminal is resized,
// demonstrating the useWindowSize hook.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, useWindowSize } from 'ink';

export default function WindowSize() {
  const { columns, rows } = useWindowSize();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Window Size</Text>
      <Box marginTop={1}>
        <Text>
          <Text bold>Columns:</Text> {columns}
        </Text>
      </Box>
      <Box>
        <Text>
          <Text bold>Rows:</Text> {rows}
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>
          Resize the terminal to see this update.
        </Text>
      </Box>
    </Box>
  );
}
