// Context advanced example — demonstrates React Context patterns with Ink.
// Shows how to share state across components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: createContext, useContext, and custom components are not supported
// in runts HIR runtime. For compatibility, we use direct object access.

import React from 'react';
import { Box, Text } from 'ink';

interface Theme {
  primary: string;
  secondary: string;
}

export default function ContextAdvancedExample() {
  // Static theme for parity testing
  const theme: Theme = {
    primary: 'cyan',
    secondary: 'gray',
  };
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Context Advanced Demo</Text>
      <Text></Text>
      
      <Text bold>Theme (object access):</Text>
      <Box flexDirection="column">
        <Text>Primary color: <Text color={theme.primary as any}>{theme.primary}</Text></Text>
        <Text>Secondary color: <Text color={theme.secondary as any}>{theme.secondary}</Text></Text>
      </Box>
      
      <Text></Text>
      <Text dimColor>Theme provides consistent styling across components.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
