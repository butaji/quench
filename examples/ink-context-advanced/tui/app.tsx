// Context advanced example — demonstrates React Context patterns with Ink.
// Shows how to share state across components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { createContext, useContext, useState } from 'react';
import { Box, Text } from 'ink';

interface Theme {
  primary: string;
  secondary: string;
}

const ThemeContext = createContext<Theme>({
  primary: 'cyan',
  secondary: 'gray',
});

function ThemedText({ children, bold }: { children: React.ReactNode; bold?: boolean }) {
  const theme = useContext(ThemeContext);
  return (
    <Text color={theme.primary as any} bold={bold}>
      {children}
    </Text>
  );
}

function ThemeInfo() {
  const theme = useContext(ThemeContext);
  return (
    <Box flexDirection="column">
      <Text>Primary: <Text color={theme.primary as any}>{theme.primary}</Text></Text>
      <Text>Secondary: <Text color={theme.secondary as any}>{theme.secondary}</Text></Text>
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
      <ThemedText bold>Context Advanced Demo</ThemedText>
      <Text></Text>
      <Text bold>Theme (from context):</Text>
      <ThemeInfo />
      <Text></Text>
      <Text dimColor>Theme provides consistent styling across components.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
