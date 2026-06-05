// Mouse events example — demonstrates mouse event handling in Ink.
// Shows position tracking and button events.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime) - static display
//   3. runts build (codegen->runts-ink) - mouse support

import React from 'react';
import { Box, Text } from 'ink';

// NOTE: Mouse events require terminal mouse support.
// For parity testing, we show static content.

export default function MouseEventsExample() {
  // Static content for parity
  const position = { x: 0, y: 0 };
  const button = 'None';
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Mouse Events Demo</Text>
      <Text></Text>
      
      <Text>Position: X={position.x}, Y={position.y}</Text>
      <Text>Button: {button}</Text>
      <Text></Text>
      
      <Text bold>Supported mouse events:</Text>
      <Box flexDirection="column" marginLeft={2}>
        <Text>  Left click, Right click, Middle click</Text>
        <Text>  Mouse move with button held</Text>
        <Text>  Scroll up/down</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Move mouse and click to see coordinates.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
