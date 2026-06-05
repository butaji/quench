// Context advanced example — demonstrates React Context with Ink components.
// Shows how to share state across deeply nested components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { createContext, useContext, useState } from 'react';
import { Box, Text } from 'ink';

// Create a theme context
interface Theme {
  primary: string;
  secondary: string;
}

const ThemeContext = createContext<Theme>({
  primary: 'cyan',
  secondary: 'gray',
});

// Consumer component
function ThemeDisplay() {
  const theme = useContext(ThemeContext);
  
  return (
    <Box flexDirection="column">
      <Text>Primary color: <Text color={theme.primary as any}>{theme.primary}</Text></Text>
      <Text>Secondary color: <Text color={theme.secondary as any}>{theme.secondary}</Text></Text>
    </Box>
  );
}

export default function ContextAdvancedExample() {
  const [theme] = useState<Theme>({
    primary: 'cyan',
    secondary: 'gray',
  });
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Context Advanced Demo</Text>
      <Text></Text>
      
      <Text bold>Theme from Context:</Text>
      <ThemeContext.Provider value={theme}>
        <ThemeDisplay />
      </ThemeContext.Provider>
      
      <Text></Text>
      <Text dimColor>Context provides dependency injection for components.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
