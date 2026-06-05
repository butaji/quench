// Context example — demonstrates React Context with Ink.
// Shows how to use context to pass data through the component tree.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { createContext, useContext } from 'react';
import { Box, Text } from 'ink';

// Theme context for demonstration
interface Theme {
  primaryColor: string;
  secondaryColor: string;
}

const ThemeContext = createContext<Theme>({
  primaryColor: 'cyan',
  secondaryColor: 'green',
});

function ThemeDisplay() {
  const theme = useContext(ThemeContext);
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Context Demo</Text>
      <Text></Text>
      <Text>Primary Color: <Text color={theme.primaryColor as any}>{theme.primaryColor}</Text></Text>
      <Text>Secondary Color: <Text color={theme.secondaryColor as any}>{theme.secondaryColor}</Text></Text>
      <Text></Text>
      <Text dimColor>Context passes data without prop drilling.</Text>
    </Box>
  );
}

export default function ContextDemo() {
  return (
    <ThemeContext.Provider value={{ primaryColor: 'magenta', secondaryColor: 'yellow' }}>
      <Box>
        <ThemeDisplay />
      </Box>
    </ThemeContext.Provider>
  );
}
