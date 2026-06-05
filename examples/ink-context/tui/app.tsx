// Context example — demonstrates React Context with Ink.
// Shows how to use context to pass data through the component tree.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — full React Context support
//   2. runts dev (HIR runtime) — static render with default values
//   3. runts build (codegen->runts-ink) — full interactivity

import React, { createContext, useContext } from 'react';
import { Box, Text } from 'ink';

interface Theme {
  primary: string;
  secondary: string;
}

const ThemeContext = createContext<Theme>({
  primary: 'cyan',
  secondary: 'green',
});

function ThemeDisplay() {
  const theme = useContext(ThemeContext);
  return (
    <Box flexDirection="column">
      <Text>Primary Color: <Text color={theme.primary as any}>{theme.primary}</Text></Text>
      <Text>Secondary Color: <Text color={theme.secondary as any}>{theme.secondary}</Text></Text>
    </Box>
  );
}

export default function ContextDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Context Demo</Text>
      <Text></Text>
      <Text bold>Theme (from context):</Text>
      <ThemeDisplay />
      <Text></Text>
      <Text dimColor>Context passes data without prop drilling.</Text>
    </Box>
  );
}
