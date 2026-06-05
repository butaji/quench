// Combined hooks example — demonstrates using multiple hooks together.
// This is a more complex app that uses useInput, useWindowSize, and useApp.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-combined-hooks
// 3. runts compile: runts build examples/ink-combined-hooks --plugin ratatui --release

import React, { useState } from 'react';
import { Box, Text, useInput, useWindowSize, useApp } from 'ink';

export default function CombinedHooksExample() {
  const [counter, setCounter] = useState(0);
  const { columns, rows } = useWindowSize();
  const { exit } = useApp();

  useInput((input, key) => {
    if (key.upArrow) {
      setCounter((c) => c + 1);
    } else if (key.downArrow) {
      setCounter((c) => Math.max(0, c - 1));
    } else if (input === 'q' || key.escape) {
      exit();
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Combined Hooks Demo</Text>
      <Text></Text>
      
      <Box gap={1}>
        <Text bold>Window Size:</Text>
        <Text>{columns}x{rows}</Text>
      </Box>
      
      <Box gap={1}>
        <Text bold>Counter:</Text>
        <Text>{counter}</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Controls:</Text>
      <Text dimColor>  Up/Down - Change counter</Text>
      <Text dimColor>  q/Esc - Exit</Text>
      
      <Text></Text>
      <Box
        flexDirection="column"
        borderStyle="round"
        padding={1}
        borderColor="cyan"
      >
        <Text>This box uses multiple features:</Text>
        <Text>  • Flexbox layout (gap, padding)</Text>
        <Text>  • Border styling</Text>
        <Text>  • Multiple hooks</Text>
      </Box>
    </Box>
  );
}
