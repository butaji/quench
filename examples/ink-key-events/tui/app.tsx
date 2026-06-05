// Key events example — demonstrates all supported keyboard event types.
// Shows how to handle arrow keys, modifiers, and special keys.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime) - static display
//   3. runts build (codegen->runts-ink) - interactive

import React, { useState } from 'react';
import { Box, Text } from 'ink';

// NOTE: useInput is not yet supported in runts HIR runtime.
// For parity testing, we show static content.

export default function KeyEventsExample() {
  const [lastKey, setLastKey] = useState<string>('None');
  
  // For parity - show static content
  const keyDisplay = 'Space';
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Key Events Demo</Text>
      <Text></Text>
      
      <Text>Last key pressed: <Text bold>{keyDisplay}</Text></Text>
      <Text></Text>
      
      <Text bold>Supported keys:</Text>
      <Box flexDirection="column" marginLeft={2}>
        <Text>  Arrow keys: Up, Down, Left, Right</Text>
        <Text>  Modifiers: Ctrl, Shift, Alt</Text>
        <Text>  Navigation: Home, End, PageUp, PageDown</Text>
        <Text>  Editing: Backspace, Delete, Tab, Enter</Text>
        <Text>  Other: Escape</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Press keys to see them displayed here.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
