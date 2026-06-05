// Combined hooks example — demonstrates using multiple hooks together.
// This is a more complex app that uses useInput, useApp, and useState.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-combined-hooks
// 3. runts compile: runts build examples/ink-combined-hooks --plugin ratatui --release

import React, { useState } from 'react';
import { Box, Text, Spacer, useInput, useApp } from 'ink';

export default function CombinedHooksExample() {
  const [counter, setCounter] = useState(0);
  const [multiplier, setMultiplier] = useState(1);
  const { exit } = useApp();

  useInput((input, key) => {
    if (key.upArrow) {
      setCounter((c) => c + multiplier);
    } else if (key.downArrow) {
      setCounter((c) => Math.max(0, c - multiplier));
    } else if (key.leftArrow) {
      setMultiplier((m) => Math.max(1, m - 1));
    } else if (key.rightArrow) {
      setMultiplier((m) => m + 1);
    } else if (input === 'q' || key.escape) {
      exit();
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Combined Hooks Demo</Text>
      <Text></Text>
      
      <Box gap={1}>
        <Text bold>Counter:</Text>
        <Text>{counter}</Text>
      </Box>
      
      <Box gap={1}>
        <Text bold>Step size:</Text>
        <Text>{multiplier}</Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Controls:</Text>
      <Text dimColor>  Up/Down - Change counter</Text>
      <Text dimColor>  Left/Right - Change step</Text>
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
        <Text>  • Multiple hooks (useInput, useApp)</Text>
        <Text>  • Multiple state variables</Text>
      </Box>
      
      <Spacer />
      
      <Box gap={1}>
        <Text dimColor>Total: </Text>
        <Text dimColor>{counter * multiplier}</Text>
      </Box>
    </Box>
  );
}
