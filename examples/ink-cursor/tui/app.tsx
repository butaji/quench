// Cursor example — demonstrates cursor positioning control.
// Shows different cursor positioning options.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, useCursor } from 'ink';

export default function CursorExample() {
  const { hideCursor, showCursor, isCursorVisible } = useCursor();
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Cursor Control Demo</Text>
      <Text></Text>
      <Text>Cursor positioning options:</Text>
      <Text></Text>
      <Box flexDirection="column">
        <Text>
          <Text cursor>▮</Text> Block cursor
        </Text>
        <Text>
          <Text dimColor>Line cursor (default)</Text>
        </Text>
        <Text>
          <Text underline>_</Text> Underscore cursor
        </Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
