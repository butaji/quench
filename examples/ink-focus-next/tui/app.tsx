// Focus navigation example — demonstrates tab-based focus navigation.
// Press Tab/Shift+Tab to navigate between focusable elements.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-focus-next
// 3. runts compile: runts build examples/ink-focus-next --plugin ratatui --release

import React, { useState } from 'react';
import { Box, Text, useInput, useFocus } from 'ink';

function FocusableBox({ id, children }: { id: string; children: React.ReactNode }) {
  const { isFocused } = useFocus({ id });
  
  return (
    <Box
      borderStyle="round"
      borderColor={isFocused ? 'cyan' : 'white'}
      paddingX={2}
      paddingY={1}
      minWidth={15}
    >
      <Text
        bold={isFocused}
        color={isFocused ? 'cyan' : 'white'}
        dimColor={!isFocused}
      >
        {children}
      </Text>
    </Box>
  );
}

export default function FocusNextExample() {
  const [selected, setSelected] = useState(0);
  const ids = ['first', 'second', 'third', 'fourth'];

  useInput((input, key) => {
    if (key.tab) {
      if (key.shift) {
        setSelected((prev) => (prev - 1 + ids.length) % ids.length);
      } else {
        setSelected((prev) => (prev + 1) % ids.length);
      }
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Navigation Demo</Text>
      <Text></Text>
      <Text>Press Tab/Shift+Tab to navigate.</Text>
      <Text dimColor>Current: {ids[selected]}</Text>
      <Text></Text>
      
      <Box gap={1} flexDirection="column">
        <Box gap={1}>
          <FocusableBox id={ids[0]}>First</FocusableBox>
          <FocusableBox id={ids[1]}>Second</FocusableBox>
        </Box>
        <Box gap={1}>
          <FocusableBox id={ids[2]}>Third</FocusableBox>
          <FocusableBox id={ids[3]}>Fourth</FocusableBox>
        </Box>
      </Box>
      
      <Text></Text>
      <Text italic dimColor>
        Focus navigation works with useFocus and useFocusManager.
      </Text>
    </Box>
  );
}
