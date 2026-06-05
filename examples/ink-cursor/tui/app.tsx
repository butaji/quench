// Cursor example — exercises the useCursor hook.
// Demonstrates cursor positioning and visibility control.
//
// The cursor can be shown/hidden and positioned at
// specific coordinates within the component.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import React, { useEffect } from 'react';
import { Box, Text, useCursor } from 'ink';

export default function Cursor() {
  const { x, y, show, hide } = useCursor();

  useEffect(() => {
    // Show cursor on mount
    show();
  }, [show]);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Cursor Example</Text>
      <Box marginTop={1}>
        <Text>
          <Text bold>Position:</Text> ({x}, {y})
        </Text>
      </Box>
      <Box marginTop={1}>
        <Text dimColor>Cursor is visible when rendered.</Text>
      </Box>
    </Box>
  );
}
